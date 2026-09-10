/*
    REFERENCES
    ----------

    1. https://learn.microsoft.com/en-us/playready/specifications/playready-header-specification

*/

use crate::{bail, error::Result};
use base64::Engine;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename = "WRMHEADER")]
pub struct WrmHeader {
    #[serde(rename = "@version")]
    version: String,
    #[serde(rename = "DATA")]
    data: Option<Data>,
}

#[derive(Deserialize)]
struct Data {
    #[serde(rename = "KID")]
    kid: Option<String>,
    #[serde(rename = "PROTECTINFO")]
    protect_info: Option<ProtectInfo>,
}

#[derive(Deserialize)]
struct ProtectInfo {
    #[serde(rename = "KID")]
    kid: Option<KeyID>,
    #[serde(rename = "KIDS", default)]
    kids: Option<KeyIDs>,
}

#[derive(Deserialize)]
struct KeyID {
    #[serde(rename = "@VALUE")]
    value: String,
}

#[derive(Deserialize)]
struct KeyIDs {
    #[serde(rename = "KID", default)]
    kids: Vec<KeyID>,
}

impl WrmHeader {
    /// Extracts Key IDs from the WRMHEADER based on its version.
    ///
    /// # Errors
    ///
    /// Returns an error if the `PlayReady` object header version is unsupported.
    pub fn key_ids(&self) -> Result<Vec<String>> {
        let mut key_ids = Vec::new();

        match self.version.as_str() {
            "4.0.0.0" => {
                if let Some(Data { kid: Some(x), .. }) = &self.data {
                    key_ids.push(x.to_owned());
                }
            }
            "4.1.0.0" => {
                if let Some(Data {
                    protect_info: Some(ProtectInfo { kid: Some(x), .. }),
                    ..
                }) = &self.data
                {
                    key_ids.push(x.value.clone());
                }
            }
            "4.2.0.0" | "4.3.0.0" => {
                if let Some(Data {
                    protect_info: Some(ProtectInfo { kid: Some(x), .. }),
                    ..
                }) = &self.data
                {
                    key_ids.push(x.value.clone());
                }

                if let Some(Data {
                    protect_info: Some(ProtectInfo { kids: Some(x), .. }),
                    ..
                }) = &self.data
                {
                    for kid in &x.kids {
                        key_ids.push(kid.value.clone());
                    }
                }
            }

            x => {
                bail!("Unsupported pssh box playready object header version v{x}.");
            }
        }

        Ok(key_ids
            .iter()
            .map(|x| hex::encode(base64::engine::general_purpose::STANDARD.decode(x).unwrap()))
            .collect())
    }
}
