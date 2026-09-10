use crate::boxes::{SencSample, SencSubsample};
use aes::{
    Aes128,
    cipher::{BlockModeDecrypt, KeyIvInit, StreamCipher},
};

type Aes128Ctr = ctr::Ctr128BE<Aes128>;
type Aes128Cbc = cbc::Decryptor<Aes128>;

enum CipherMode {
    Cenc,
    Cens,
    Cbc1,
    Cbcs,
    None,
}

pub struct CencProcessor {
    key: [u8; 16],
    iv: [u8; 16],
    crypt_size: usize,
    skip_size: usize,
    mode: CipherMode,
}

impl CencProcessor {
    pub const fn new(key: &[u8; 16], crypt_blocks: u8, skip_blocks: u8, scheme_type: u32) -> Self {
        Self {
            key: *key,
            iv: [0u8; 16],
            crypt_size: crypt_blocks as usize * 16,
            skip_size: skip_blocks as usize * 16,
            mode: match scheme_type {
                0x63656E63 => CipherMode::Cenc,
                0x63656E73 => CipherMode::Cens,
                0x63626331 => CipherMode::Cbc1,
                0x63626373 => CipherMode::Cbcs,
                _ => CipherMode::None,
            },
        }
    }

    pub fn decrypt_sample_inplace(&mut self, data: &mut [u8], sample: &SencSample) {
        if matches!(self.mode, CipherMode::None) {
            return;
        }

        self.iv = sample.iv;

        if !sample.subsamples.is_empty() {
            self.decrypt_subsamples_inplace(data, &sample.iv, &sample.subsamples);
        } else if let CipherMode::Cbc1 | CipherMode::Cbcs = self.mode {
            self.decrypt_full_blocks_inplace(data);
        } else {
            self.process_inplace(data);
        }
    }

    fn decrypt_subsamples_inplace(
        &mut self,
        data: &mut [u8],
        iv: &[u8; 16],
        subsamples: &[SencSubsample],
    ) {
        match self.mode {
            CipherMode::Cenc | CipherMode::Cens => {
                self.decrypt_subsamples_ctr_inplace(data, iv, subsamples);
            }
            _ => {
                self.decrypt_subsamples_cbc_inplace(data, iv, subsamples);
            }
        }
    }

    fn decrypt_full_blocks_inplace(&mut self, data: &mut [u8]) {
        let blocks = (data.len() / 16) * 16;
        if blocks > 0 {
            self.process_inplace(&mut data[..blocks]);
        }
    }

    fn process_inplace(&mut self, data: &mut [u8]) {
        match self.mode {
            CipherMode::Cenc => self.process_ctr_inplace(data),
            CipherMode::Cens => self.process_cens_pattern_inplace(data),
            CipherMode::Cbc1 => self.process_cbc_inplace(data),
            CipherMode::Cbcs => self.process_cbcs_pattern_inplace(data),
            CipherMode::None => (),
        }
    }

    fn decrypt_subsamples_ctr_inplace(
        &self,
        data: &mut [u8],
        iv: &[u8; 16],
        subsamples: &[SencSubsample],
    ) {
        let mut cipher = Aes128Ctr::new((&self.key).into(), iv.into());
        let has_pattern =
            self.crypt_size > 0 && self.skip_size > 0 && matches!(self.mode, CipherMode::Cens);
        let len = data.len();
        let mut offset = 0;

        for sub in subsamples {
            let clear_size = sub.bytes_of_clear_data as usize;
            let enc_size = sub.bytes_of_encrypted_data as usize;

            if offset + clear_size + enc_size > len {
                return;
            }

            let start = offset + clear_size;

            if enc_size > 0 {
                if has_pattern {
                    let mut pat_offset = 0;
                    while pat_offset < enc_size {
                        let to_crypt = (enc_size - pat_offset).min(self.crypt_size);
                        if to_crypt > 0 {
                            cipher.apply_keystream(
                                &mut data[start + pat_offset..start + pat_offset + to_crypt],
                            );
                            pat_offset += to_crypt;
                        }
                        if pat_offset >= enc_size {
                            break;
                        }
                        let to_skip = (enc_size - pat_offset).min(self.skip_size);
                        pat_offset += to_skip;
                    }
                } else {
                    cipher.apply_keystream(&mut data[start..start + enc_size]);
                }
            }

            offset += clear_size + enc_size;
        }
    }

    fn decrypt_subsamples_cbc_inplace(
        &mut self,
        data: &mut [u8],
        iv: &[u8; 16],
        subsamples: &[SencSubsample],
    ) {
        let len = data.len();
        let mut offset = 0;

        for sub in subsamples {
            let clear_size = sub.bytes_of_clear_data as usize;
            let enc_size = sub.bytes_of_encrypted_data as usize;

            if offset + clear_size + enc_size > len {
                self.iv = *iv;
                self.process_inplace(&mut data[offset..]);
                return;
            }

            if enc_size > 0 {
                if matches!(self.mode, CipherMode::Cbcs) {
                    self.iv = *iv;
                }
                let start = offset + clear_size;
                self.process_inplace(&mut data[start..start + enc_size]);
            }

            offset += clear_size + enc_size;
        }
    }

    fn process_ctr_inplace(&self, data: &mut [u8]) {
        Aes128Ctr::new((&self.key).into(), (&self.iv).into()).apply_keystream(data);
    }

    fn process_cens_pattern_inplace(&self, data: &mut [u8]) {
        if self.crypt_size == 0 && self.skip_size == 0 {
            self.process_ctr_inplace(data);
            return;
        }

        let len = data.len();
        let mut cipher = Aes128Ctr::new((&self.key).into(), (&self.iv).into());
        let mut offset = 0;

        while offset < len {
            let to_encrypt = (len - offset).min(self.crypt_size);
            if to_encrypt > 0 {
                cipher.apply_keystream(&mut data[offset..offset + to_encrypt]);
                offset += to_encrypt;
            }

            if offset >= len {
                break;
            }

            let to_skip = (len - offset).min(self.skip_size);
            offset += to_skip;
        }
    }

    fn process_cbc_inplace(&self, data: &mut [u8]) {
        let blocks = (data.len() / 16) * 16;
        if blocks == 0 {
            return;
        }

        Aes128Cbc::new((&self.key).into(), (&self.iv).into())
            .decrypt_padded::<cipher::block_padding::NoPadding>(&mut data[..blocks])
            .unwrap();
    }

    fn process_cbcs_pattern_inplace(&mut self, data: &mut [u8]) {
        if self.crypt_size == 0 && self.skip_size == 0 {
            self.process_cbc_inplace(data);
            return;
        }

        let len = data.len();
        let mut offset = 0;
        while offset < len {
            let to_encrypt = (len - offset).min(self.crypt_size);
            let blocks = (to_encrypt / 16) * 16;

            if blocks > 0 {
                let iv_start = offset + blocks - 16;
                let mut next_iv = [0u8; 16];
                next_iv.copy_from_slice(&data[iv_start..iv_start + 16]);

                self.process_cbc_inplace(&mut data[offset..offset + blocks]);
                self.iv = next_iv;
            }

            offset += to_encrypt;

            if offset >= len {
                break;
            }

            let to_skip = (len - offset).min(self.skip_size);
            offset += to_skip;
        }
    }
}
