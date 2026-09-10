use crate::{
    core::{fetch, mux::Muxer, sub, vid},
    error::Result,
    format::{FormatExpr, SelectType},
    playlist::{ClipRange, MasterPlaylist, MediaPlaylist, MediaType},
    progress::Progress,
    utils,
};
use colored::Colorize;
use log::{info, warn};
use reqwest::{Client, Url};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
};
use tokio_util::sync::CancellationToken;
use vsd_mp4::{boxes::TencBox, pssh::PsshBox};

/// Configuration options for the playlist download process.
#[derive(Clone)]
pub struct PlaylistDownloadConfig {
    /// The HTTP client used for fetching streams and segments.
    pub client: Client,
    /// Whether to automatically attempt decryption of streams.
    pub decrypt: bool,
    /// The temporary/download directory.
    pub directory: Option<PathBuf>,
    /// Cryptographic keys used for decryption (Key ID -> Key mapping).
    pub keys: HashMap<String, String>,
    /// Whether to merge all downloaded tracks into a single output file.
    pub merge: bool,
    /// Query parameters appended to request URLs.
    pub query: Arc<Vec<(String, String)>>,
    /// Whether to resume partial downloads.
    pub resume: bool,
    /// Number of retries per chunk/segment on network failure.
    pub retries: u8,
    /// Number of concurrent segment download tasks.
    pub threads: u8,
}

/// A downloader for HLS and DASH playlist streams.
///
/// `PlaylistDownloader` allows configuring the download behavior (threads, retries, filters, etc.),
/// parsing the manifest, fetching segments concurrently, decrypting, and muxing the output files.
pub struct PlaylistDownloader {
    base_url: Option<Url>,
    clip: Option<ClipRange>,
    config: PlaylistDownloadConfig,
    format_expr: FormatExpr,
    output: Option<PathBuf>,
    select_type: SelectType,
    subs_codec: String,
}

impl PlaylistDownloader {
    /// Creates a new [`PlaylistDownloader`] with defaults.
    #[must_use]
    pub fn new(client: &Client) -> Self {
        Self {
            base_url: None,
            clip: None,
            format_expr: FormatExpr::parse("b+s+allund").unwrap(),
            output: None,
            select_type: SelectType::None,
            subs_codec: "copy".to_owned(),
            config: PlaylistDownloadConfig {
                client: client.clone(),
                decrypt: true,
                directory: None,
                keys: HashMap::new(),
                merge: true,
                query: Arc::new(Vec::new()),
                resume: true,
                retries: 10,
                threads: 5,
            },
        }
    }

    /// Sets the base URL for resolving relative links.
    pub fn base_url(mut self, base_url: impl Into<Url>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Sets the clip range to download a subset of the playlist (e.g. `01:00-01:30`).
    ///
    /// # Errors
    /// Returns an error if the clip range string is invalid.
    pub fn clip(mut self, clip: &str) -> Result<Self> {
        self.clip = Some(ClipRange::new(clip)?);
        Ok(self)
    }

    /// Sets whether to attempt decrypting the streams (default: `true`).
    #[must_use]
    pub const fn decrypt(mut self, decrypt: bool) -> Self {
        self.config.decrypt = decrypt;
        self
    }

    /// Sets the temporary download directory (default: `.`).
    pub fn directory(mut self, directory: impl Into<PathBuf>) -> Self {
        self.config.directory = Some(directory.into());
        self
    }
    /// Sets the format expression for stream selection (default: `"bv+ba+s"`).
    ///
    /// # Errors
    /// Returns an error if the format expression is invalid.
    pub fn format(mut self, format: &str) -> Result<Self> {
        self.format_expr = FormatExpr::parse(format)?;
        Ok(self)
    }

    /// Configures the stream selection interface.
    ///
    /// If `raw` is `true`, a basic text list prompt is displayed;
    /// otherwise, a modern interactive multi-select prompt is used.
    #[must_use]
    pub const fn interactive(mut self, raw: bool) -> Self {
        if raw {
            self.select_type = SelectType::Raw;
        } else {
            self.select_type = SelectType::Modern;
        }
        self
    }

    /// Sets the decryption keys (`key_id` hex -> key hex).
    #[must_use]
    pub fn keys(mut self, keys: HashMap<String, String>) -> Self {
        self.config.keys = keys;
        self
    }

    /// Sets whether to merge downloaded streams into a single output file (default: `true`).
    #[must_use]
    pub const fn merge(mut self, merge: bool) -> Self {
        self.config.merge = merge;
        self
    }

    /// Sets the path to the output file.
    pub(crate) fn output(mut self, output: impl Into<PathBuf>) -> Self {
        self.output = Some(output.into());
        self
    }

    /// Sets query parameters to append to requests.
    #[must_use]
    pub fn query(mut self, query: &str) -> Self {
        if query.is_empty() {
            return self;
        }
        self.config.query = Arc::new(
            query
                .trim_start_matches('?')
                .split('&')
                .filter_map(|x| {
                    if let Some((key, value)) = x.split_once('=') {
                        Some((key.to_owned(), value.to_owned()))
                    } else {
                        None
                    }
                })
                .collect(),
        );
        self
    }

    /// Sets whether to resume partial downloads (default: `true`).
    #[must_use]
    pub const fn resume(mut self, resume: bool) -> Self {
        self.config.resume = resume;
        self
    }

    /// Sets the maximum retry count per segment download (default: `10`).
    #[must_use]
    pub const fn retries(mut self, retries: u8) -> Self {
        self.config.retries = retries;
        self
    }

    /// Sets subtitle codec for muxing (default: `"copy"`).
    pub(crate) fn subs_codec(mut self, subs_codec: impl Into<String>) -> Self {
        self.subs_codec = subs_codec.into();
        self
    }

    /// Sets concurrent segment download thread count (default: `5`, clamped between 1 and 16).
    #[must_use]
    pub fn threads(mut self, threads: u8) -> Self {
        self.config.threads = threads.clamp(1, 16);
        self
    }

    /// Gets a reference to the download configuration.
    #[must_use]
    pub const fn get_config(&self) -> &PlaylistDownloadConfig {
        &self.config
    }

    /// Fetches and parses the playlist manifest.
    ///
    /// If `partial_parse` is `true`, it applies format selection and interactive prompts
    /// to determine which streams should be processed.
    ///
    /// # Errors
    /// Returns an error if fetching or parsing the playlist fails.
    pub async fn parse(&self, uri: &str, partial_parse: bool) -> Result<MasterPlaylist> {
        let fp = fetch::playlist(&self.config, &self.base_url, uri).await?;
        let mut mp = if partial_parse {
            fp.parse(&self.config, &self.format_expr, &self.select_type, true)
                .await?
        } else {
            fp.parse(&self.config, &self.format_expr, &SelectType::None, false)
                .await?
        };

        if let Some(clip) = &self.clip {
            mp.clip_streams(clip);
        }

        Ok(mp)
    }

    /// Parses the playlist manifest, downloads all selected stream segments, decrypts them,
    /// and muxes them into the final output file using `ffmpeg`.
    ///
    /// # Errors
    /// Returns an error if:
    /// - Parsing, downloading, or decryption fails.
    /// - `ffmpeg` is not installed or cannot be found during muxing.
    pub(crate) async fn download(self, uri: &str) -> Result<()> {
        let mp = self.parse(uri, true).await?;
        let streams = mp.streams;

        if self.config.decrypt {
            dump_pssh_info(&self.config, &streams).await?;
        }

        let token = CancellationToken::new();
        let ctrlc_token = token.clone();

        let ctrlc_handle = tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() && !ctrlc_token.is_cancelled() {
                warn!("Aborting download due to ctrl+c.");
                ctrlc_token.cancel();
            }

            if tokio::signal::ctrl_c().await.is_ok() {
                warn!("Force exiting due to ctrl+c.");
                std::process::exit(1);
            }
        });

        let mut muxer = Muxer::new();
        let total = streams.len();

        for (i, stream) in streams.iter().enumerate() {
            info!(
                "DownLd [{}] {}",
                stream.media_type.to_string().green(),
                stream.display().cyan(),
            );

            if stream.segments.is_empty() {
                warn!("Stream skipped because no segments were found.");
                continue;
            }

            let progress =
                Progress::new(&format!("{}/{}", i + 1, total), stream.segments.len(), None);

            if stream.media_type == MediaType::Subtitles {
                muxer.push(sub::download(&self.config, progress, &token, stream).await?);
            } else {
                muxer.push(vid::download(&self.config, progress, &token, stream).await?);
            }
        }

        ctrlc_handle.abort();

        if let Some(output) = &self.output
            && muxer.should_mux(&self.config)
        {
            let Some(ffmpeg) = utils::find_ffmpeg() else {
                bail!(
                    "ffmpeg couldn't be located, it's required to continue further. Download it from https://github.com/Tyrrrz/FFmpegBin/releases"
                );
            };
            muxer.mux(&ffmpeg, output, &self.subs_codec).await?;
            muxer.clean(self.config.directory.as_deref()).await?;
        }

        Ok(())
    }
}

/// Parses initialization segments from the given streams to extract and dump DRM PSSH and Key ID information.
///
/// # Errors
/// Returns an error if downloading or parsing initialization segments/boxes fails.
pub async fn dump_pssh_info(
    config: &PlaylistDownloadConfig,
    streams: &[MediaPlaylist],
) -> Result<()> {
    let mut default_kids = HashSet::new();
    let mut init_segments = Vec::new();

    for stream in streams {
        let Some(bytes) = stream.fetch_init(config).await? else {
            continue;
        };

        if let Some(key_id) = TencBox::from_init(&bytes)?.and_then(|x| {
            let key_id = x.default_kid_hex();
            if key_id == "00000000000000000000000000000000" {
                stream.default_kid()
            } else {
                Some(key_id)
            }
        }) {
            default_kids.insert(key_id);
        }

        init_segments.push(bytes);
    }

    let mut boxes = HashSet::new();
    let mut key_ids = HashSet::new();

    for bytes in &init_segments {
        let pssh = PsshBox::from_init(bytes)?;

        for pssh in pssh.boxes {
            let pssh_base64 = pssh.as_base64();
            if !boxes.insert(pssh_base64.clone()) {
                continue;
            }

            info!(
                "DrmPsh [{}] {}",
                pssh.system_id.to_string().magenta(),
                pssh_base64,
            );
            for key_id in &pssh.key_ids {
                if !key_ids.insert(key_id.clone()) {
                    continue;
                }

                info!(
                    "DrmKid [{}] {}{}",
                    pssh.system_id.to_string().magenta(),
                    key_id,
                    if default_kids.contains(key_id) {
                        " (required)".bold().red()
                    } else {
                        "".normal()
                    },
                );
            }
        }
    }

    Ok(())
}
