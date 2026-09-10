use clap::Parser;
use log::{error, warn};
use vsd::{Args, Error};

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    if let Err(e) = Args::parse().execute().await {
        match e {
            Error::DownloadInterrupted => {
                warn!("{}", e);
            }
            _ => {
                error!("{}", e);
                std::process::exit(1);
            }
        }
    }
}
