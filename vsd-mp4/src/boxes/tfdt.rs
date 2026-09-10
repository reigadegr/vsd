use crate::{ParsedBox, Result};

/// Track Fragment Base Media Decode Time Box (tfdt) - absolute decode time of the first sample.
///
/// This box provides the absolute decode time, measured on the media timeline,
/// of the first sample in decode order in the track fragment.
#[derive(Debug, Clone)]
pub struct TfdtBox {
    /// The absolute decode time, measured on the media timeline, of the first sample in decode order in the track fragment.
    pub base_media_decode_time: u64,
}

impl TfdtBox {
    /// Parses a `tfdt` box from a `ParsedBox`.
    pub fn new(box_: &mut ParsedBox) -> Result<Self> {
        let reader = &mut box_.reader;
        let version = box_.version.unwrap();

        Ok(Self {
            base_media_decode_time: if version == 1 {
                reader.read_u64()?
            } else {
                u64::from(reader.read_u32()?)
            },
        })
    }
}
