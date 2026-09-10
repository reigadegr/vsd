use crate::error::{Error, Result};
use clap::{Args, ValueEnum};
use std::path::PathBuf;
use tokio::{
    fs::{self, File},
    io::{self, AsyncWriteExt, BufReader},
    process::Command,
};

/// Merge multiple media segments into a single file.
#[derive(Args, Clone, Debug)]
pub struct Merge {
    /// List of input files e.g. *.ts, segment_*.m4s, etc.
    #[arg(required = true)]
    input: Vec<String>,

    /// Output file path for the merged file.
    #[arg(short, long, required = true)]
    output: PathBuf,

    /// Merge strategy to use.
    ///
    /// binary: raw byte concatenation.
    /// ffmpeg: use concat demuxer for container aware merging.
    #[arg(short = 't', long = "type", value_enum, default_value_t = MergeType::Binary)]
    typ: MergeType,
}

#[derive(Debug, Clone, ValueEnum)]
enum MergeType {
    Binary,
    Ffmpeg,
}

impl Merge {
    pub async fn execute(self) -> Result<()> {
        let output_canonical = self.output.canonicalize().ok();
        let files = self
            .input
            .iter()
            .filter_map(|p| glob::glob(p).ok())
            .flatten()
            .filter_map(std::result::Result::ok)
            .filter(|f| {
                if let Some(out) = output_canonical.as_ref() {
                    return f.canonicalize().ok().as_ref() != Some(out);
                }
                true
            })
            .collect::<Vec<_>>();

        if files.len() < 2 {
            bail!("At least two files are required to perform a merge.");
        }

        match self.typ {
            MergeType::Binary => {
                const BUFFER_SIZE: usize = 1024 * 1024 * 10;
                let mut output = File::create(self.output).await?;

                for path in files {
                    let file = File::open(path).await?;
                    let mut input = BufReader::with_capacity(BUFFER_SIZE, file);
                    io::copy(&mut input, &mut output).await?;
                }

                output.flush().await?;
            }
            MergeType::Ffmpeg => {
                let content = files
                    .iter()
                    .map(|f| format!("file '{}'", f.to_string_lossy().replace('\'', "'\\''")))
                    .collect::<Vec<_>>()
                    .join("\n");

                fs::write("vsd-merge.txt", content).await?;
                let status = Command::new("ffmpeg")
                    .args([
                        "-hide_banner",
                        "-loglevel",
                        "error",
                        "-y",
                        "-f",
                        "concat",
                        "-safe",
                        "0",
                        "-i",
                        "vsd-merge.txt",
                        "-c",
                        "copy",
                        &self.output.to_string_lossy(),
                    ])
                    .status()
                    .await?;
                fs::remove_file("vsd-merge.txt").await?;

                if !status.success() {
                    return Err(Error::FfmpegFailed {
                        code: status.code().unwrap_or(1),
                        message: "concat demuxer failed.".into(),
                    });
                }
            }
        }

        Ok(())
    }
}
