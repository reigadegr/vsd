use crate::{
    core::{PlaylistDownloadConfig, mux::Stream},
    error::{Error, Result},
    playlist::MediaPlaylist,
    progress::Progress,
    utils,
};
use colored::Colorize;
use log::{debug, info, trace, warn};
use reqwest::{Url, header};
use tokio::{
    fs::{self, File},
    io::AsyncWriteExt,
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;
use vsd_mp4::sub::{StppSubsParser, WvttSubsParser, ttml};

enum SubtitleType {
    Mp4Vtt,
    Mp4Ttml,
    SrtText,
    TtmlText,
    Unknown,
    VttText,
}

fn detect_codec(codecs: Option<&str>, data: &[u8], ext: &str) -> (&'static str, SubtitleType) {
    if let Some(codecs) = codecs {
        match codecs.to_lowercase().as_str() {
            "vtt" => return ("vtt", SubtitleType::VttText),
            "wvtt" => return ("vtt", SubtitleType::Mp4Vtt),
            "stpp" | "stpp.ttml" | "stpp.ttml.im1t" => return ("srt", SubtitleType::Mp4Ttml),
            _ => (),
        }
    }

    if data.starts_with(b"WEBVTT") || ext == "vtt" {
        ("vtt", SubtitleType::VttText)
    } else if data.starts_with(b"1") || ext == "srt" {
        ("srt", SubtitleType::SrtText)
    } else if data.starts_with(b"<?xml") || data.starts_with(b"<tt") || ext == "ttml" {
        ("srt", SubtitleType::TtmlText)
    } else if WvttSubsParser::from_init(data).is_ok() {
        ("vtt", SubtitleType::Mp4Vtt)
    } else if StppSubsParser::from_init(data).is_ok() {
        ("srt", SubtitleType::Mp4Ttml)
    } else {
        warn!("Stream uses unknown subtitle codec.");
        ("txt", SubtitleType::Unknown)
    }
}

pub async fn download(
    config: &PlaylistDownloadConfig,
    progress: Progress,
    token: &CancellationToken,
    stream: &MediaPlaylist,
) -> Result<Stream> {
    if let Some(dir) = &config.directory
        && !dir.exists()
    {
        fs::create_dir_all(dir).await?;
    }

    let base_url = stream.uri.parse::<Url>()?;
    let ext = stream.extension();
    let mut data = Vec::new();
    let mut temp_file = stream.path(config.directory.as_ref());

    if let Some(mut bytes) = stream.fetch_init(config).await? {
        data.append(&mut bytes);
    }

    let segment = &stream.segments[0];
    let url = base_url.join(&segment.uri)?;
    let mut request = config.client.get(url.clone()).query(&*config.query);

    if let Some(range) = &segment.range {
        request = request.header(header::RANGE, range);
    }

    trace!(
        "Fetching {} (segment@{})",
        url,
        segment
            .range
            .as_ref()
            .map(|x| format!("{}-{}", x.0, x.1))
            .as_deref()
            .unwrap_or("full-range")
    );

    let response = request.send().await?;
    let mut bytes = utils::fetch_bytes(response).await?;
    let size = bytes.len();
    data.append(&mut bytes);

    let (ext, codec) = detect_codec(stream.codecs.as_deref(), &data, ext);

    temp_file = temp_file.with_extension(ext);
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
        temp_file.to_string_lossy()
    );

    progress.update(size);

    let remaining = &stream.segments[1..];

    if !remaining.is_empty() {
        let progress_handle = progress.spawn();
        let max_threads = config.threads as usize;
        let mut set: JoinSet<Result<(usize, Vec<u8>)>> = JoinSet::new();
        let mut results = vec![None; remaining.len()];

        for (i, segment) in remaining.iter().enumerate() {
            if token.is_cancelled() {
                break;
            }

            while set.len() >= max_threads {
                if let Some(Ok(result)) = set.join_next().await {
                    let (i, bytes) = match result {
                        Ok(v) => v,
                        Err(e) => {
                            set.abort_all();
                            return Err(e);
                        }
                    };
                    progress.update(bytes.len());
                    results[i] = Some(bytes);
                }
            }

            let client = config.client.clone();
            let query = config.query.clone();
            let range = segment.range.clone();
            let url = base_url.join(&segment.uri)?;

            set.spawn(async move {
                trace!(
                    "Fetching {} (segment@{})",
                    url,
                    range
                        .as_ref()
                        .map(|x| format!("{}-{}", x.0, x.1))
                        .as_deref()
                        .unwrap_or("full-range")
                );

                let mut request = client.get(url).query(&*query);
                if let Some(range) = &range {
                    request = request.header(header::RANGE, range);
                }

                let response = request.send().await?;
                let bytes = utils::fetch_bytes(response).await?;
                Ok((i, bytes))
            });
        }

        while let Some(Ok(result)) = set.join_next().await {
            let (i, bytes) = match result {
                Ok(v) => v,
                Err(e) => {
                    set.abort_all();
                    return Err(e);
                }
            };
            progress.update(bytes.len());
            results[i] = Some(bytes);
        }

        for mut bytes in results.into_iter().flatten() {
            data.append(&mut bytes);
        }

        progress_handle.abort();
    }

    progress.finish();

    if token.is_cancelled() {
        return Err(Error::DownloadInterrupted);
    }

    let output = match codec {
        SubtitleType::Mp4Vtt => {
            debug!("Extracting wvtt subtitles.");
            let vtt = WvttSubsParser::from_init(&data)?;
            vtt.parse(&data, None)?.as_vtt().into_bytes()
        }
        SubtitleType::Mp4Ttml => {
            debug!("Extracting stpp subtitles.");
            let ttml = StppSubsParser::from_init(&data)?;
            ttml.parse(&data)?.as_srt().into_bytes()
        }
        SubtitleType::TtmlText => {
            debug!("Extracting ttml+xml subtitles.");
            ttml::parse_bytes(&data)
                .map_err(|e| Error::Other(e.to_string()))?
                .into_subtitles()
                .as_srt()
                .into_bytes()
        }
        _ => data,
    };

    File::create(&temp_file).await?.write_all(&output).await?;

    Ok(temp_stream)
}
