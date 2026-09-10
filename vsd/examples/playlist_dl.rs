// This example shows using vsd as library in other rust project.
// Also see https://github.com/clitic/vsd/blob/main/vsd/src/core/playlist.rs
//
// [dependencies]
// vsd = { version = "0.5", default-features = false, features = ["rustls-tls"]}

use std::{io::Write, sync::Arc};
use vsd::{
    Error, Muxer, PlaylistDownloader, Result,
    playlist::MediaType,
    progress::{ByteSize, Eta, ProgressCallback, ProgressState},
    reqwest::Client,
    tokio,
    tokio_util::sync::CancellationToken,
};

struct Progress;

impl ProgressCallback for Progress {
    fn on_progress(&self, state: &ProgressState) {
        let stderr = std::io::stderr();
        let mut handle = stderr.lock();
        write!(
            handle,
            "\r\x1B[2K[{}/~{}({:.0}%) PT:{}/{} DL:{} ETA:{}]",
            ByteSize(state.downloaded_bytes),
            ByteSize(state.estimated_bytes),
            state.percent,
            state.downloaded_parts,
            state.total_parts,
            ByteSize(state.speed_bps as usize),
            Eta(state.eta_seconds)
        )
        .unwrap();
        handle.flush().unwrap();
    }

    fn on_finish(&self, state: &ProgressState) {
        self.on_progress(state);
        eprintln!();
    }
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    let client = Client::new();

    // We use ./target as temporary directory for downloaded files.
    let dl = PlaylistDownloader::new(&client).directory("target");
    let config = dl.get_config();

    let mp = dl
        .parse(
            "https://media.axprod.net/TestVectors/Dash/not_protected_dash_1080p_h264/manifest.mpd",
            false,
        )
        .await?;

    // You can clone this token and call .cancel() to pause a download.
    let token = CancellationToken::new();
    let mut muxer = Muxer::new();

    // Download first subtitle stream.
    for stream in mp.streams {
        if stream.media_type == MediaType::Subtitles {
            println!(
                "Downloading {} subtitles",
                stream.language.as_deref().unwrap_or("unknown")
            );

            // We download to a temporary file, mux it in, and then clean up.
            // You could also just move the file after download and avoid muxing if you want.
            let dl_info = match stream.download(config, Arc::new(Progress), &token).await {
                Ok(info) => info,
                Err(Error::MissingSegments) => {
                    println!("Stream has no segments");
                    continue;
                }
                Err(Error::UnsupportedEncryption(e)) => {
                    println!("Unsupported encryption {e}");
                    continue;
                }
                Err(Error::MissingKey(key_id)) => {
                    println!("Missing decryption key for {key_id}");
                    continue;
                }
                Err(Error::DownloadInterrupted) => {
                    println!("Download paused");
                    std::process::exit(0);
                }
                Err(e) => return Err(e),
            };

            println!("Downloaded {}", dl_info.path.to_string_lossy());
            muxer.push(dl_info);

            break;
        }
    }

    println!("Muxing to target/output.srt");
    muxer
        .mux(&vsd::find_ffmpeg().unwrap(), "target/output.srt", "srt")
        .await?;
    muxer.clean(config.directory.as_deref()).await?;

    Ok(())
}
