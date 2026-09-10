/*
    REFERENCES
    ----------

    1. https://github.com/shaka-project/shaka-player/blob/4e933116984beb630d31ce7a0b8c9bc6f8b48c06/lib/util/pssh.js
    2. https://github.com/shaka-project/shaka-packager/blob/56e227267c9091a0f65b4d92d9064dda4557f3a7/packager/tools/pssh/pssh-box.py
    3. https://github.com/shaka-project/shaka-player/blob/b441518943241693fa2df03196be6ee707c8511e/lib/dash/content_protection.js
    4. https://github.com/rlaphoenix/pywidevine/blob/master/pywidevine/pssh.py

*/

use crate::{
    bail, data,
    error::Result,
    parser,
    parser::{Mp4Parser, ParsedBox},
    pssh::{playready, widevine},
};
use base64::Engine;

const COMMON_SYSTEM_ID: &str = "1077efecc0b24d02ace33c1e52e2fb4b";
const PLAYREADY_SYSTEM_ID: &str = "9a04f07998404286ab92e65be0885f95";
const WIDEVINE_SYSTEM_ID: &str = "edef8ba979d64acea3c827dcd51d21ed";

/// Parsed PSSH box content containing PSSH data chunks from an MP4 initialization segment.
#[derive(Debug)]
pub struct PsshBox {
    /// The collection of parsed PSSH data blocks.
    pub boxes: Vec<PsshData>,
}

impl PsshBox {
    /// Parses PSSH boxes from an MP4 initialization or media segment.
    ///
    /// # Errors
    ///
    /// Returns an error if box parsing fails, if required PSSH boxes are malformed,
    /// or if there are unrecognized PSSH versions.
    pub fn from_init(data: &[u8]) -> Result<Self> {
        let boxes = data!(Vec::new());

        Mp4Parser::new()
            .base_box("moov", parser::children)
            .base_box("moof", parser::children)
            .full_box("pssh", {
                let boxes = boxes.clone();
                move |mut box_| {
                    Self::parse(&mut box_, &mut boxes.borrow_mut())?;
                    Ok(())
                }
            })
            .parse(data, false, false)?;

        Ok(Self {
            boxes: boxes.take(),
        })
    }

    fn parse(box_: &mut ParsedBox, boxes: &mut Vec<PsshData>) -> Result<()> {
        let Some(box_version) = box_.version else {
            bail!("PSSH boxes are full boxes and must have a valid version.");
        };

        if box_.flags.is_none() {
            bail!("PSSH boxes are full boxes and must have a valid flag.");
        }

        if box_version > 1 {
            bail!("Unrecognized PSSH version found!");
        }

        let system_id = hex::encode(box_.reader.read_bytes_u8(16)?);

        if box_version > 0 {
            let mut data = PsshData {
                data: box_.full_data(),
                key_ids: Vec::new(),
                system_id: if system_id == COMMON_SYSTEM_ID {
                    SystemId::Common
                } else {
                    SystemId::Other(system_id.clone())
                },
            };
            let num_key_ids = box_.reader.read_u32()?;

            for _ in 0..num_key_ids {
                let key_id = hex::encode(box_.reader.read_bytes_u8(16)?);
                data.key_ids.push(key_id);
            }

            boxes.push(data);
        }

        let pssh_data_size = box_.reader.read_u32()?;
        let pssh_data = box_.reader.read_bytes_u8(pssh_data_size as usize)?;
        let mut key_ids = Vec::new();

        match system_id.as_str() {
            PLAYREADY_SYSTEM_ID => key_ids = playready::parse_key_ids(&pssh_data)?,
            WIDEVINE_SYSTEM_ID => key_ids = widevine::parse_key_ids(&pssh_data)?,
            _ => (),
        }

        boxes.push(PsshData {
            data: box_.full_data(),
            key_ids,
            system_id: match system_id.as_str() {
                PLAYREADY_SYSTEM_ID => SystemId::PlayReady,
                WIDEVINE_SYSTEM_ID => SystemId::WideVine,
                _ => SystemId::Other(system_id.clone()),
            },
        });
        Ok(())
    }
}

/// The DRM system identifier used in a PSSH box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemId {
    /// Common `SystemID` (e.g. W3C Common Encryption).
    Common,
    /// Custom or unrecognized system identifier.
    Other(String),
    /// Microsoft `PlayReady` `SystemID`.
    PlayReady,
    /// Google Widevine `SystemID`.
    WideVine,
}

impl std::fmt::Display for SystemId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Common => "cen",
                Self::Other(x) => x,
                Self::PlayReady => "prd",
                Self::WideVine => "wvd",
            }
        )
    }
}

/// The PSSH payload parsed from a `pssh` box, containing its binary data and keys.
#[derive(Debug, Clone)]
pub struct PsshData {
    /// The full raw binary data of the parsed PSSH box.
    pub data: Vec<u8>,
    /// The hex-encoded Key IDs extracted from the PSSH box or payload.
    pub key_ids: Vec<String>,
    /// The DRM system associated with this PSSH box.
    pub system_id: SystemId,
}

impl PartialEq for PsshData {
    fn eq(&self, other: &Self) -> bool {
        self.data == other.data
    }
}

impl PsshData {
    /// Encodes the binary PSSH box data into a base64 string.
    #[must_use]
    pub fn as_base64(&self) -> String {
        base64::engine::general_purpose::STANDARD.encode(&self.data)
    }
}
