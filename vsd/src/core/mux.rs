use crate::{
    core::PlaylistDownloadConfig,
    error::{Error, Result},
    playlist::MediaType,
};
use colored::Colorize;
use log::{debug, info, warn};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::{fs, process::Command};

/// Represents a single media stream track (video, audio, or subtitle) ready for muxing.
pub struct Stream {
    /// The language tag of the stream track (e.g., `"en"`, `"es"`).
    pub language: Option<String>,
    /// The type of media content.
    pub media_type: MediaType,
    /// The path to the downloaded local temporary media file.
    pub path: PathBuf,
}

/// A wrapper around a collection of [`Stream`]s to be merged.
pub struct Muxer(Vec<Stream>);

impl std::ops::Deref for Muxer {
    type Target = Vec<Stream>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for Muxer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Muxer {
    /// Creates a new empty [`Muxer`].
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self(Vec::new())
    }

    /// Evaluates if stream merging should proceed based on the download configurations.
    ///
    /// Muxing is skipped if decryption is disabled, merging is disabled, or multiple video streams are present.
    pub fn should_mux(&self, config: &PlaylistDownloadConfig) -> bool {
        if !config.decrypt {
            warn!("--output is ignored when --no-decrypt is used.");
            return false;
        }
        if !config.merge {
            warn!("--output is ignored when --no-merge is used.");
            return false;
        }
        if self
            .0
            .iter()
            .filter(|x| x.media_type == MediaType::Video)
            .count()
            > 1
        {
            warn!("--output is ignored when multiple vid streams are selected.");
            return false;
        }
        true
    }

    /// Muxes the media tracks together into a single file container using the `ffmpeg` tool.
    ///
    /// # Errors
    ///
    /// Returns [`Error::FfmpegFailed`] if the `ffmpeg` execution returns a non-zero exit status code.
    pub async fn mux(
        &self,
        ffmpeg: impl AsRef<Path>,
        output: impl AsRef<Path>,
        subs_codec: &str,
    ) -> Result<()> {
        let ffmpeg = ffmpeg.as_ref();
        let output = output.as_ref();
        let temp_files = [
            MediaType::Video,
            MediaType::Audio,
            MediaType::Subtitles,
            MediaType::Undefined,
        ]
        .iter()
        .flat_map(|mt| self.0.iter().filter(move |x| x.media_type == *mt))
        .collect::<Vec<_>>();

        if temp_files.is_empty() {
            bail!("No streams available for muxing.");
        }

        let mut args = vec!["-hide_banner".to_owned(), "-y".to_owned()];

        for temp_file in &temp_files {
            args.extend_from_slice(&["-i".to_owned(), temp_file.path.to_string_lossy().into()]);
        }

        if temp_files.len() > 1 {
            for i in 0..temp_files.len() {
                args.extend_from_slice(&["-map".to_owned(), i.to_string()]);
            }

            let mut aud_idx = 0u8;
            let mut sub_idx = 0u8;

            for temp_file in &temp_files {
                match temp_file.media_type {
                    MediaType::Audio => {
                        if let Some(language) = &temp_file.language {
                            args.extend_from_slice(&[
                                format!("-metadata:s:a:{aud_idx}"),
                                format!("language={language}"),
                            ]);
                        }
                        aud_idx += 1;
                    }
                    MediaType::Subtitles => {
                        if let Some(language) = &temp_file.language {
                            args.extend_from_slice(&[
                                format!("-metadata:s:s:{sub_idx}"),
                                format!("language={language}"),
                            ]);
                        }
                        sub_idx += 1;
                    }
                    _ => (),
                }
            }

            if aud_idx > 0 {
                args.extend_from_slice(&["-disposition:a:0".to_owned(), "default".to_owned()]);
            }

            if sub_idx > 0 {
                args.extend_from_slice(&["-disposition:s:0".to_owned(), "default".to_owned()]);
            }
        }

        args.extend_from_slice(&[
            "-c:v".to_owned(),
            "copy".to_owned(),
            "-c:a".to_owned(),
            "copy".to_owned(),
        ]);

        let has_sub = temp_files
            .iter()
            .any(|x| x.media_type == MediaType::Subtitles);

        if has_sub {
            let codec = if subs_codec == "copy" {
                match output.extension().and_then(|e| e.to_str()) {
                    Some("mp4" | "m4v" | "mov") => "mov_text",
                    Some("srt") => "srt",
                    Some("vtt") => "webvtt",
                    Some("ass" | "ssa") => "ass",
                    _ => "copy",
                }
            } else {
                subs_codec
            };
            args.extend_from_slice(&["-c:s".to_owned(), codec.to_owned()]);
        }

        args.push(output.to_string_lossy().into());

        info!(
            "Muxing [{}] ffmpeg {}",
            "exe".green(),
            args.iter()
                .map(|x| if x.contains(' ') {
                    format!("\"{x}\"")
                } else {
                    x.to_owned()
                })
                .collect::<Vec<_>>()
                .join(" ")
        );

        let result = Command::new(ffmpeg)
            .args(&args)
            .stderr(Stdio::piped())
            .output()
            .await?;

        if !result.status.success() {
            let stderr = String::from_utf8_lossy(&result.stderr);
            return Err(Error::FfmpegFailed {
                code: result.status.code().unwrap_or(1),
                message: stderr.lines().last().unwrap_or("unknown").to_owned(),
            });
        }

        Ok(())
    }

    /// Deletes the temporary video, audio, and subtitle stream segment files and optional clean directory.
    pub async fn clean(&self, directory: Option<&Path>) -> Result<()> {
        for stream in &self.0 {
            if stream.path.exists() {
                debug!("Deleting {} file.", stream.path.to_string_lossy());
                fs::remove_file(&stream.path).await?;
            }
        }
        if let Some(directory) = directory
            && directory.read_dir()?.next().is_none()
        {
            debug!("Deleting {} directory.", directory.to_string_lossy());
            fs::remove_dir(directory).await?;
        }
        Ok(())
    }
}
