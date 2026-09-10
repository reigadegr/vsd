use cipher::{BlockModeDecrypt, KeyIvInit};

type Aes128Cbc = cbc::Decryptor<aes::Aes128>;

/// A decrypter for HTTP Live Streaming (HLS) AES-128 encrypted segments.
///
/// In HLS AES-128, the entire segment file is encrypted using AES-128 in Cipher Block
/// Chaining (CBC) mode with PKCS7 padding.
#[derive(Clone)]
pub struct HlsAes128Decrypter {
    key: [u8; 16],
    iv: [u8; 16],
}

impl HlsAes128Decrypter {
    /// Creates a new `HlsAes128Decrypter` with the given key and IV.
    #[must_use]
    pub const fn new(key: &[u8; 16], iv: &[u8; 16]) -> Self {
        Self { key: *key, iv: *iv }
    }

    /// Increments the initialization vector (IV) by 1.
    pub const fn increment_iv(&mut self) {
        self.iv = (u128::from_be_bytes(self.iv) + 1).to_be_bytes();
    }

    /// Decrypts the given encrypted segment.
    #[must_use]
    pub fn decrypt(&self, mut input: Vec<u8>) -> Vec<u8> {
        let slice_len = {
            let slice = Aes128Cbc::new((&self.key).into(), (&self.iv).into())
                .decrypt_padded::<cipher::block_padding::Pkcs7>(&mut input)
                .unwrap();
            slice.len()
        };

        input.truncate(slice_len);
        input
    }
}

/// A decrypter for HTTP Live Streaming (HLS) SAMPLE-AES encrypted segments.
///
/// In HLS SAMPLE-AES, only media samples (e.g., individual audio or video frames) are
/// encrypted, while container metadata and structural headers remain unencrypted.
#[derive(Clone)]
pub struct HlsSampleAesDecrypter {
    key: [u8; 16],
    iv: [u8; 16],
}

impl HlsSampleAesDecrypter {
    /// Creates a new `HlsSampleAesDecrypter` with the given key and IV.
    #[must_use]
    pub const fn new(key: &[u8; 16], iv: &[u8; 16]) -> Self {
        Self { key: *key, iv: *iv }
    }

    /// Increments the initialization vector (IV) by 1.
    pub const fn increment_iv(&mut self) {
        self.iv = (u128::from_be_bytes(self.iv) + 1).to_be_bytes();
    }

    /// Decrypts the given encrypted segment.
    #[must_use]
    pub fn decrypt(&self, input: Vec<u8>) -> Vec<u8> {
        let mut input = std::io::Cursor::new(input);
        let mut output = Vec::new();
        iori_ssa::decrypt(&mut input, &mut output, self.key, self.iv).unwrap();
        output
    }
}
