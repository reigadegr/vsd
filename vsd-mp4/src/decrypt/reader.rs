use std::io::{ErrorKind, Read, Result};

pub struct Mp4Reader {
    pub box_type: [u8; 4],
    pub header_bytes: Vec<u8>,
    pub total_size: u64,
}

impl Mp4Reader {
    pub fn header<R: Read>(reader: &mut R) -> Result<Option<Self>> {
        let mut buf = [0u8; 8];
        match reader.read_exact(&mut buf) {
            Ok(()) => (),
            Err(e) if e.kind() == ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e),
        }

        let size = u64::from(u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]));
        let box_type = [buf[4], buf[5], buf[6], buf[7]];

        if size == 1 {
            let mut ext = [0u8; 8];
            reader.read_exact(&mut ext)?;
            let total_size = u64::from_be_bytes(ext);
            let mut header = Vec::with_capacity(16);
            header.extend_from_slice(&buf);
            header.extend_from_slice(&ext);
            Ok(Some(Self {
                box_type,
                total_size,
                header_bytes: header,
            }))
        } else {
            Ok(Some(Self {
                box_type,
                header_bytes: buf.to_vec(),
                total_size: size,
            }))
        }
    }

    pub fn data<R: Read>(&self, reader: &mut R) -> Result<Vec<u8>> {
        if self.total_size == 0 {
            let mut data = Vec::from(self.header_bytes.as_slice());
            reader.read_to_end(&mut data)?;
            return Ok(data);
        }

        let body_size = self.total_size as usize - self.header_bytes.len();
        let mut data = vec![0u8; self.total_size as usize];
        data[..self.header_bytes.len()].copy_from_slice(&self.header_bytes);
        if body_size > 0 {
            reader.read_exact(&mut data[self.header_bytes.len()..])?;
        }
        Ok(data)
    }

    pub fn init<R: Read>(reader: &mut R) -> Result<(Vec<u8>, Option<Self>)> {
        let mut init = Vec::new();

        while let Some(header) = Self::header(reader)? {
            if &header.box_type == b"moof" {
                return Ok((init, Some(header)));
            }

            init.append(&mut header.data(reader)?);
        }

        Ok((init, None))
    }
}
