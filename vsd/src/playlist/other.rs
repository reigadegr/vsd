use crate::{
    core::PlaylistDownloadConfig,
    error::Result,
    playlist::types::{Key, Range},
    utils,
};
use log::debug;
use reqwest::{Url, header::HeaderValue};

impl TryFrom<&Range> for HeaderValue {
    type Error = reqwest::header::InvalidHeaderValue;

    fn try_from(range: &Range) -> std::result::Result<Self, Self::Error> {
        Self::from_str(&format!("bytes={}-{}", range.0, range.1))
    }
}

impl Key {
    pub async fn key(&self, config: &PlaylistDownloadConfig, base_url: &Url) -> Result<[u8; 16]> {
        let url = base_url.join(self.uri.as_ref().unwrap())?;
        debug!("Fetching {url} (key@full-range)");
        let response = config.client.get(url).query(&*config.query).send().await?;
        let bytes = utils::fetch_bytes(response).await?;
        Ok(bytes.as_slice().try_into()?)
    }

    pub fn iv(&self, sequence: u64) -> Result<[u8; 16]> {
        self.iv
            .as_ref()
            .map(|iv| {
                u128::from_str_radix(iv.trim_start_matches("0x").trim_start_matches("0X"), 16)
                    .map(u128::to_be_bytes)
            })
            .transpose()?
            .map_or(Ok(u128::from(sequence).to_be_bytes()), Ok)
    }
}
