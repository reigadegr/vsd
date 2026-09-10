use crate::error::{Error, Result};
use reqwest::Response;
use std::{env, path::PathBuf};

pub async fn fetch_bytes(response: Response) -> Result<Vec<u8>> {
    let status = response.status();

    if !status.is_success() {
        return Err(Error::RequestFailed {
            url: response.url().to_string(),
            status,
            body: response.text().await?,
        });
    }

    Ok(response.bytes().await?.to_vec())
}

/// Discovers the absolute path of the `ffmpeg` executable binary.
///
/// Looks in the current working directory, the parent directory of the current executable,
/// and directories listed in the system's `PATH` environment variable.
#[must_use]
pub fn find_ffmpeg() -> Option<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(path) = env::current_dir() {
        paths.push(path);
    }
    if let Some(path) = env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
    {
        paths.push(path);
    }
    if let Some(path) = env::var_os("PATH") {
        paths.extend(env::split_paths(&path));
    }
    #[cfg(target_os = "windows")]
    let bin = "ffmpeg.exe";
    #[cfg(not(target_os = "windows"))]
    let bin = "ffmpeg";
    paths.into_iter().map(|x| x.join(bin)).find(|x| x.exists())
}

/// Generates a short, unique 7-character hexadecimal identifier.
///
/// Computes a BLAKE3 cryptographic hash of the combined base URL and resource URI.
#[must_use]
pub fn gen_id(base_url: &str, uri: &str) -> String {
    blake3::hash(format!("{base_url}+{uri}").as_bytes()).to_hex()[..7].to_owned()
}
