use crate::{Mp4Parser, ParsedBox, Result, bail, data};

/// A byte range for a subsegment or media fragment indexed by the `sidx` box.
#[derive(Debug, Clone)]
pub struct SidxRange {
    /// The ending byte offset (inclusive) of the subsegment.
    pub end: u64,
    /// The starting byte offset of the subsegment.
    pub start: u64,
}

/// Segment Index Box (sidx) - provides index information for media subsegments.
///
/// This box defines the subsegment structure, mapping them to specific byte ranges
/// inside the media container.
#[derive(Debug, Clone)]
pub struct SidxBox {
    /// The list of byte ranges for the indexed subsegments.
    pub ranges: Vec<SidxRange>,
}

impl SidxBox {
    /// Helper method to find and parse a `sidx` box from initialization or segment data.
    ///
    /// # Arguments
    /// * `data` - The byte slice containing the box hierarchy.
    /// * `offset` - The absolute starting byte offset of the `sidx` box.
    pub fn from_init(data: &[u8], offset: u64) -> Result<Option<Self>> {
        let sidx_box = data!();
        let sidx_box_c = sidx_box.clone();

        Mp4Parser::new()
            .full_box("sidx", move |mut box_| {
                *sidx_box_c.borrow_mut() = Some(Self::new(&mut box_, offset)?);
                Ok(())
            })
            .parse(data, false, false)?;

        Ok(sidx_box.take())
    }

    /// Parses a `sidx` box from a `ParsedBox`.
    ///
    /// # Arguments
    /// * `box_` - The parsed box to read from.
    /// * `offset` - The absolute starting byte offset of the `sidx` box.
    pub fn new(box_: &mut ParsedBox, offset: u64) -> Result<Self> {
        if box_.version.is_none() {
            bail!("SIDX is a full box and should have a valid version.");
        }

        let reader = &mut box_.reader;
        let version = box_.version.unwrap();

        let mut references = Vec::new();

        reader.skip(4)?;

        let timescale = reader.read_u32()?;

        if timescale == 0 {
            bail!("SIDX box has invalid timescale.");
        }

        let (_earliest_presentation_time, first_offset) = if version == 0 {
            (u64::from(reader.read_u32()?), u64::from(reader.read_u32()?))
        } else {
            (reader.read_u64()?, reader.read_u64()?)
        };

        reader.skip(2)?;

        let reference_count = reader.read_u16()?;

        // Subtract the presentation time offset
        // let mut unscaled_start_time = earliest_presentation_time;
        let mut start_byte = offset + box_.size as u64 + first_offset;

        for _ in 0..reference_count {
            // |chunk| is 1 bit for |referenceType|, and 31 bits for |referenceSize|.
            let chunk = reader.read_u32()?;
            let reference_type = (chunk & 0x80000000) >> 31;
            let reference_size = chunk & 0x7FFFFFFF;

            let _subsegment_duration = reader.read_u32()?;

            // Skipping 1 bit for |startsWithSap|, 3 bits for |sapType|, and 28 bits
            // for |sapDelta|.
            reader.skip(4)?;

            // If |referenceType| is 1 then the reference is to another SIDX.
            // We do not support this.
            if reference_type == 1 {
                bail!("Hierarchical SIDXs are not supported.");
            }

            // The media timestamps inside the container.
            // let native_start_Time = unscaled_start_time as f64 / timescale as f64;
            // let native_end_Time = (unscaled_start_time as f64 + subsegment_duration as f64) / timescale as f64;

            references.push(SidxRange {
                end: start_byte + u64::from(reference_size) - 1,
                start: start_byte,
            });

            // unscaled_start_time += subsegment_duration as u64;
            start_byte += u64::from(reference_size);
        }

        box_.parser.stop();
        Ok(Self { ranges: references })
    }
}
