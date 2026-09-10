use crate::{
    Mp4Parser,
    boxes::{SchmBox, SencBox, TencBox, TrunBox},
    data,
    decrypt::{cipher::CencProcessor, reader::Mp4Reader},
    error::{Error, Result},
    parser,
};
use std::io::{Read, Write};

#[derive(Clone, Default)]
struct Tenc {
    scheme_type: u32,
    per_sample_iv_size: u8,
    constant_iv: Option<[u8; 16]>,
    crypt_byte_block: u8,
    skip_byte_block: u8,
}

/// A decrypter for Common Encryption (CENC) protected ISO Base Media File Format (ISOBMFF) streams.
///
/// `CencDecrypter` provides utilities to decrypt fragmented MP4 files or individual fragments
/// encrypted using common encryption schemes (e.g., `cenc`, `cbcs`, `cens`, `cbc1`).
#[derive(Clone)]
pub struct CencDecrypter {
    key: [u8; 16],
    tenc: Option<Tenc>,
}

impl CencDecrypter {
    /// Creates a new `CencDecrypter` with a 16-byte decryption key.
    ///
    /// The `key` must be a hexadecimal string representing the 16-byte decryption key.
    ///
    /// # Errors
    ///
    /// Returns an error if the hex string is invalid or if the decoded key is not exactly 16 bytes.
    pub fn new(key: &str) -> Result<Self> {
        Ok(Self {
            key: hex::decode(key.to_ascii_lowercase().replace('-', ""))?
                .try_into()
                .map_err(|v: Vec<u8>| Error::InvalidKeySize(v.len()))?,
            tenc: None,
        })
    }

    /// Creates a new `CencDecrypter` with a decryption key and pre-parse initialization data.
    ///
    /// The initialization data (`init`) represents the MP4 metadata/header (e.g., `moov` box),
    /// which is parsed to extract the track encryption (`tenc`) and scheme type (`schm`) parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if the key is invalid or if parsing the initialization data fails.
    pub fn with_init(key: &str, init: &[u8]) -> Result<Self> {
        let mut decrypter = Self::new(key)?;
        decrypter.tenc = Some(Self::parse_init(init)?);
        Ok(decrypter)
    }

    fn parse_init(init: &[u8]) -> Result<Tenc> {
        let tenc = data!(Tenc::default());

        Mp4Parser::new()
            .base_box("enca", parser::audio_sample_entry)
            .base_box("encv", parser::visual_sample_entry)
            .base_box("mdia", parser::children)
            .base_box("minf", parser::children)
            .base_box("moov", parser::children)
            .base_box("schi", parser::children)
            .base_box("sinf", parser::children)
            .base_box("stbl", parser::children)
            .full_box("stsd", parser::sample_description)
            .base_box("trak", parser::children)
            .full_box("schm", {
                let tenc = tenc.clone();
                move |mut box_| {
                    tenc.borrow_mut().scheme_type = SchmBox::new(&mut box_)?.scheme_type;
                    Ok(())
                }
            })
            .full_box("tenc", {
                let tenc = tenc.clone();
                move |mut box_| {
                    let tenc_box = TencBox::new(&mut box_)?;
                    let t = &mut *tenc.borrow_mut();
                    t.per_sample_iv_size = tenc_box.per_sample_iv_size;
                    t.constant_iv = tenc_box.constant_iv;
                    t.crypt_byte_block = tenc_box.crypt_byte_block;
                    t.skip_byte_block = tenc_box.skip_byte_block;
                    box_.parser.stop();
                    Ok(())
                }
            })
            .parse(init, true, true)?;

        Ok(tenc.take())
    }

    /// Decrypts a single MP4 fragment in-place and returns the decrypted fragment.
    ///
    /// A fragment typically consists of a movie fragment box (`moof`) followed by a media data box (`mdat`).
    ///
    /// If `init` is provided, its track encryption parameters are parsed and used for decryption.
    /// Otherwise, cached parameters are used. If both are missing, it attempts to parse
    /// the track encryption parameters directly from the `input` fragment.
    ///
    /// # Limitations
    ///
    /// * **Single-Track Assumption**: The function assumes that the fragment contains media data
    ///   for a single track, or that all encrypted tracks share the same encryption parameters.
    ///   It does not differentiate between tracks using track IDs (e.g., `track_id` in `tenc`, `traf`,
    ///   or `tfhd` boxes is ignored), and applies the parsed track encryption parameters to all samples
    ///   sequentially.
    ///
    /// # Errors
    ///
    /// Returns an error if parsing the fragment, parsing the initialization data, or decryption fails.
    pub fn decrypt_fragment(&self, mut input: Vec<u8>, init: Option<&[u8]>) -> Result<Vec<u8>> {
        if input.is_empty() {
            return Ok(input);
        }

        let tenc_own;
        let tenc = if let Some(init) = init {
            tenc_own = Self::parse_init(init)?;
            &tenc_own
        } else if let Some(cached) = &self.tenc {
            cached
        } else {
            tenc_own = Self::parse_init(&input)?;
            &tenc_own
        };

        if tenc.scheme_type == 0 {
            return Ok(input);
        }

        #[derive(Default)]
        struct State {
            start: u64,
            senc: Option<SencBox>,
            trun: Option<TrunBox>,
        }
        let state = data!(State::default());
        let iv_size = tenc.per_sample_iv_size;
        let constant_iv = tenc.constant_iv;

        Mp4Parser::new()
            .base_box("traf", parser::children)
            .base_box("moof", {
                let state = state.clone();
                move |box_| {
                    state.borrow_mut().start = box_.start;
                    parser::children(box_)
                }
            })
            .full_box("senc", {
                let state = state.clone();
                move |mut box_| {
                    state.borrow_mut().senc =
                        Some(SencBox::new(&mut box_, iv_size, constant_iv.as_ref())?);
                    Ok(())
                }
            })
            .full_box("trun", {
                let state = state.clone();
                move |mut box_| {
                    state.borrow_mut().trun = Some(TrunBox::new(&mut box_)?);
                    Ok(())
                }
            })
            .parse(&input, true, true)?;

        let state = state.take();
        let (Some(trun), Some(senc)) = (state.trun, state.senc) else {
            return Ok(input);
        };
        let mut processor = CencProcessor::new(
            &self.key,
            tenc.crypt_byte_block,
            tenc.skip_byte_block,
            tenc.scheme_type,
        );
        let mut offset = (state.start + u64::from(trun.data_offset.unwrap_or(0))) as usize;
        let output_len = input.len();

        for (trun_sample, senc_sample) in trun.sample_data.iter().zip(senc.samples.iter()) {
            let size = trun_sample.sample_size.unwrap_or_default() as usize;
            if size == 0 {
                continue;
            }

            let end = offset + size;
            if end > output_len {
                break;
            }

            processor.decrypt_sample_inplace(&mut input[offset..end], senc_sample);
            offset = end;
        }

        Ok(input)
    }

    /// Decrypts a fragmented MP4 stream from a reader and writes the decrypted output to a writer.
    ///
    /// If `init` is provided, it is parsed for track encryption parameters. Otherwise, the decrypter
    /// will read the initialization data from the beginning of the stream.
    ///
    /// # Limitations
    ///
    /// * **Fragment Box Order**: It expects each fragment to consist of a `moof` box followed
    ///   eventually by an `mdat` box. While typical for DASH/HLS fragmented streams, streams with
    ///   complex interleaving or out-of-order boxes might not be handled correctly.
    /// * **Unfragmented (Progressive) Streams**: Only fragmented MP4 streams or individual fragments
    ///   are decrypted. If an unfragmented MP4 stream is processed (indicated by the absence of a `moof`
    ///   box), it is written to the output unmodified without attempting decryption.
    ///
    /// # Errors
    ///
    /// Returns an error if reading from the stream, parsing metadata, decrypting, or writing fails.
    pub fn decrypt_stream<R: Read, W: Write>(
        &mut self,
        reader: &mut R,
        writer: &mut W,
        init: Option<&[u8]>,
    ) -> Result<()> {
        let mut next = if let Some(init) = init {
            self.tenc = Some(Self::parse_init(init)?);
            Mp4Reader::header(reader)?
        } else {
            let (init, moof) = Mp4Reader::init(reader)?;
            writer.write_all(&init)?;

            if moof.is_none() {
                std::io::copy(reader, writer)?;
                return Ok(());
            }

            self.tenc = Some(Self::parse_init(&init)?);
            moof
        };

        while let Some(header) = next {
            if &header.box_type == b"moof" {
                let mut fragment = header.data(reader)?;

                while let Some(next) = Mp4Reader::header(reader)? {
                    fragment.append(&mut next.data(reader)?);

                    if &next.box_type == b"mdat" {
                        break;
                    }
                }

                let decrypted = self.decrypt_fragment(fragment, None)?;
                writer.write_all(&decrypted)?;
            } else {
                writer.write_all(&header.data(reader)?)?;
            }

            next = Mp4Reader::header(reader)?;
        }

        writer.flush()?;
        Ok(())
    }
}
