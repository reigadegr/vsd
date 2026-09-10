use crate::{
    core::{
        PlaylistDownloadConfig,
        enc::{self, Decrypter},
        file::CHUNK_SIZE,
        mux::Stream,
    },
    error::{Error, Result},
    playlist::{KeyMethod, MediaPlaylist, Range, Segment},
    progress::Progress,
};
use colored::Colorize;
use log::{debug, info, trace, warn};
use reqwest::{StatusCode, Url, header};
use std::sync::Arc;
use tokio::{
    fs::{self, File},
    io::{self, AsyncWriteExt},
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;
use vsd_mp4::{
    boxes::TencBox,
    decrypt::{CencDecrypter, HlsAes128Decrypter, HlsSampleAesDecrypter},
    pssh::PsshBox,
};

const PNG_HEADER: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

pub async fn download(
    config: &PlaylistDownloadConfig,
    progress: Progress,
    token: &CancellationToken,
    stream: &MediaPlaylist,
) -> Result<Stream> {
    enc::check_unsupported(stream)?;

    if let Some(dir) = &config.directory
        && !dir.exists()
    {
        fs::create_dir_all(dir).await?;
    }

    let temp_file = stream.path(config.directory.as_ref());
    let temp_stream = Stream {
        language: stream.language.clone(),
        media_type: stream.media_type.clone(),
        path: temp_file.clone(),
    };

    if temp_file.exists() && config.resume {
        info!(
            "Saving [{}] {} (downloaded)",
            stream.media_type.to_string().green(),
            temp_file.to_string_lossy()
        );
        let size = fs::metadata(&temp_file).await.map_or(0, |x| x.len());
        progress.update_total(1);
        progress.update(size as usize);
        progress.finish();
        return Ok(temp_stream);
    }
    info!(
        "Saving [{}] {}",
        stream.media_type.to_string().green(),
        temp_file.with_extension("").to_string_lossy()
    );

    let base_url = stream.uri.parse::<Url>()?;
    let ext = stream.extension();
    let max_threads = config.threads as usize;
    let progress_handle = progress.spawn();
    let temp_dir = temp_file.with_extension("");
    let mut auto_increment_iv = false;
    let mut decrypter = Decrypter::None;
    let mut set = JoinSet::new();

    let init = stream.fetch_init(config).await?;

    let default_kid = if let Some(init) = &init {
        TencBox::from_init(init)?.map(|x| x.default_kid_hex())
    } else {
        stream.default_kid()
    };

    if !config.resume && temp_dir.exists() {
        fs::remove_dir_all(&temp_dir).await?;
    }
    fs::create_dir_all(&temp_dir).await?;

    if let Some(init) = &init {
        let mut f = File::create(temp_dir.join("init.mp4")).await?;
        f.write_all(init).await?;
        f.flush().await?;
    }

    let segments = if stream.segments.len() == 1 {
        &split_single_seg(config, &base_url, &stream.segments[0]).await?
    } else {
        &stream.segments
    };
    progress.update_total(segments.len());

    for (i, segment) in segments.iter().enumerate() {
        if token.is_cancelled() {
            break;
        }

        while set.len() >= max_threads {
            if let Some(Ok(result)) = set.join_next().await {
                match result {
                    Ok(bytes) => progress.update(bytes),
                    Err(e) => {
                        set.abort_all();
                        return Err(e);
                    }
                }
            }
        }

        if config.decrypt {
            if decrypter.is_hls()
                && segment.key.is_none()
                && auto_increment_iv
                && stream.segments.len() > 1
            {
                decrypter.increment_iv();
            }

            if let Some(key) = &segment.key {
                let media_sequence = stream.media_sequence + i as u64;

                match key.method {
                    KeyMethod::Aes128 => {
                        decrypter = Decrypter::Aes128(HlsAes128Decrypter::new(
                            &key.key(config, &base_url).await?,
                            &key.iv(media_sequence)?,
                        ));
                        auto_increment_iv = key.iv.is_none();
                    }
                    KeyMethod::SampleAes => {
                        decrypter = Decrypter::SampleAes(HlsSampleAesDecrypter::new(
                            &key.key(config, &base_url).await?,
                            &key.iv(media_sequence)?,
                        ));
                        auto_increment_iv = key.iv.is_none();
                    }
                    KeyMethod::Cenc if !matches!(decrypter, Decrypter::Cenc(_)) => {
                        let default_kid = default_kid.as_ref().ok_or_else(|| {
                            Error::Other("Unable to determine default kid for this stream.".into())
                        })?;

                        if config.keys.is_empty() {
                            return Err(Error::MissingKey(default_kid.to_owned()));
                        }

                        let key = if let Some(v) = config.keys.get(default_kid) {
                            v.to_owned()
                        } else {
                            warn!(
                                "No key provided for {default_kid}, checking pssh data for other mappable kids."
                            );
                            let mut found = None;

                            if let Some(bytes) = &init {
                                for kid in PsshBox::from_init(bytes)?
                                    .boxes
                                    .into_iter()
                                    .flat_map(|x| x.key_ids)
                                {
                                    if let Some(v) = config.keys.get(&kid) {
                                        found = Some(v.to_owned());
                                        break;
                                    }
                                }
                            }

                            found.ok_or_else(|| {
                                Error::Other("Unable to determine key for this stream.".into())
                            })?
                        };

                        info!("DrmKey [{}] {}:{}", "dec".magenta(), default_kid, key);
                        decrypter = Decrypter::Cenc(Arc::new(if let Some(init) = &init {
                            CencDecrypter::with_init(&key, init)?
                        } else {
                            CencDecrypter::new(&key)?
                        }));
                    }
                    _ => (),
                }
            }
        }

        let temp_file = temp_dir.join(format!("{i}.{ext}.part"));
        let out_file = temp_file.with_extension("");

        if out_file.exists() {
            let size = fs::metadata(&out_file).await?.len();
            progress.skip(size as usize);
            continue;
        }

        let client = config.client.clone();
        let decrypter = decrypter.clone();
        let max_retries = config.retries;
        let range = segment.range.clone();
        let url = base_url.join(&segment.uri)?;
        let query = config.query.clone();

        set.spawn(async move {
            let range_label = range
                .as_ref()
                .map_or("full-range".to_owned(), |x| format!("{}-{}", x.0, x.1));

            trace!("Fetching {url} (segment@{range_label})");
            let mut last_err = None;
            let mut bytes = None;

            for attempt in 0..=max_retries {
                if attempt > 0 {
                    trace!("ReFetching {url} (segment@{range_label})");
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }

                let mut request = client.get(url.clone()).query(&*query);
                if let Some(range) = &range {
                    request = request.header(header::RANGE, range);
                }

                match request.send().await {
                    Ok(response) => {
                        let status = response.status();

                        if status.is_success() {
                            bytes = Some(response.bytes().await?.to_vec());
                            break;
                        }

                        last_err = Some(Error::RequestFailed {
                            url: url.to_string(),
                            status,
                            body: response.text().await?,
                        });
                    }
                    Err(e) => {
                        last_err = Some(Error::RequestFailed {
                            url: url.to_string(),
                            status: e.status().unwrap_or(StatusCode::default()),
                            body: "GET".to_owned(),
                        });
                    }
                }
            }

            let mut bytes = bytes.ok_or_else(|| {
                last_err.unwrap_or(Error::Other(format!(
                    "{url} download failed after max retries."
                )))
            })?;
            let size = bytes.len();

            // Trim fake PNG header.
            if bytes.len() >= 8 && bytes[..8] == PNG_HEADER {
                bytes.drain(..8);
            }

            let bytes = decrypter.decrypt(bytes)?;

            let mut f = File::create(&temp_file).await?;
            f.write_all(&bytes).await?;
            f.flush().await?;
            fs::rename(&temp_file, temp_file.with_extension("")).await?;

            Ok(size)
        });
    }

    while let Some(Ok(result)) = set.join_next().await {
        match result {
            Ok(bytes) => progress.update(bytes),
            Err(e) => {
                set.abort_all();
                return Err(e);
            }
        }
    }

    progress_handle.abort();
    progress.finish();

    if token.is_cancelled() {
        return Err(Error::DownloadInterrupted);
    }

    if config.merge {
        info!(
            "Concat [{}] {}",
            stream.media_type.to_string().green(),
            temp_file.to_string_lossy()
        );

        let mut output = File::create(temp_file).await?;
        let init_path = temp_dir.join("init.mp4");

        if init_path.exists() {
            io::copy(&mut File::open(&init_path).await?, &mut output).await?;
        }

        for i in 0..segments.len() {
            let path = temp_dir.join(format!("{i}.{ext}"));

            if path.exists() {
                io::copy(&mut File::open(&path).await?, &mut output).await?;
            }
        }

        debug!("Deleting {} directory.", temp_dir.to_string_lossy());
        fs::remove_dir_all(&temp_dir).await?;
    } else {
        debug!("Merging skipped for this stream.");
    }

    Ok(temp_stream)
}

async fn split_single_seg(
    config: &PlaylistDownloadConfig,
    base_url: &Url,
    segment: &Segment,
) -> Result<Vec<Segment>> {
    let url = base_url.join(&segment.uri)?;
    debug!("Fetching {url} (segment@head)");
    let response = config
        .client
        .head(url.clone())
        .query(&*config.query)
        .send()
        .await?;
    let status = response.status();

    if !status.is_success() {
        return Err(Error::RequestFailed {
            url: url.to_string(),
            status,
            body: "HEAD".to_owned(),
        });
    }

    let content_length = response
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    if content_length == 0 {
        return Ok(vec![segment.clone()]);
    }

    let mut map = segment.map.clone();
    let mut key = segment.key.clone();
    let mut segments = Vec::new();

    for start in (0..content_length).step_by(CHUNK_SIZE as usize) {
        let end = (start + CHUNK_SIZE - 1).min(content_length - 1);
        segments.push(Segment {
            map: map.take(),
            key: key.take(),
            duration: segment.duration,
            range: Some(Range(start, end)),
            uri: segment.uri.clone(),
        });
    }

    Ok(segments)
}
