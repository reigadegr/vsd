use crate::{
    error::{Error, Result},
    playlist::{Key, KeyMethod, MediaPlaylist, Segment},
};
use std::sync::Arc;
use vsd_mp4::decrypt::{CencDecrypter, HlsAes128Decrypter, HlsSampleAesDecrypter};

#[derive(Clone)]
pub enum Decrypter {
    Aes128(HlsAes128Decrypter),
    Cenc(Arc<CencDecrypter>),
    SampleAes(HlsSampleAesDecrypter),
    None,
}

impl Decrypter {
    pub const fn is_hls(&self) -> bool {
        matches!(self, Self::Aes128(_) | Self::SampleAes(_))
    }

    pub const fn increment_iv(&mut self) {
        match self {
            Self::Aes128(processor) => processor.increment_iv(),
            Self::SampleAes(processor) => processor.increment_iv(),
            _ => (),
        }
    }

    pub fn decrypt(&self, input: Vec<u8>) -> Result<Vec<u8>> {
        Ok(match self {
            Self::Cenc(processor) => processor.decrypt_fragment(input, None)?,
            Self::Aes128(processor) => processor.decrypt(input),
            Self::SampleAes(processor) => processor.decrypt(input),
            Self::None => input,
        })
    }
}

pub fn check_unsupported(stream: &MediaPlaylist) -> Result<()> {
    if let Some(Segment {
        key:
            Some(
                Key {
                    method: KeyMethod::Other(x),
                    ..
                },
                ..,
            ),
        ..
    }) = stream.segments.first()
    {
        return Err(Error::UnsupportedEncryption(x.to_owned()));
    }

    Ok(())
}
