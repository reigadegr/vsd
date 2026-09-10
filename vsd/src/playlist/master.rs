use crate::{
    core::PlaylistDownloadConfig,
    error::Result,
    format::{self, FormatExpr, SelectType},
    playlist::types::{MasterPlaylist, MediaType, StreamMetadata},
};
use std::cmp::Reverse;
use vsd_mp4::{boxes::TencBox, pssh::PsshBox};

impl MasterPlaylist {
    pub(crate) fn sort_streams(mut self) -> Self {
        let mut vid_streams = Vec::new();
        let mut aud_streams = Vec::new();
        let mut sub_streams = Vec::new();
        let mut und_streams = Vec::new();

        for stream in self.streams {
            match stream.media_type {
                MediaType::Video => vid_streams.push(stream),
                MediaType::Audio => aud_streams.push(stream),
                MediaType::Subtitles => sub_streams.push(stream),
                MediaType::Undefined => und_streams.push(stream),
            }
        }

        vid_streams.sort_by_key(|s| {
            let pixels = s.resolution.map_or(0, |(w, h)| w * h);
            let bandwidth = s.bandwidth.unwrap_or_default();
            Reverse((pixels, bandwidth))
        });

        aud_streams.sort_by_key(|s| {
            let channels = (s.channels.unwrap_or_default() * 10.0) as u32;
            let bandwidth = s.bandwidth.unwrap_or_default();
            Reverse((channels, bandwidth))
        });

        self.streams = vid_streams
            .into_iter()
            .chain(aud_streams)
            .chain(sub_streams)
            .chain(und_streams)
            .collect();

        self
    }

    pub(crate) fn select_streams(
        mut self,
        format_expr: &FormatExpr,
        select_type: &SelectType,
    ) -> Result<Self> {
        let selected = format_expr.eval(&self.streams);
        self.streams = format::select(self.streams, &selected, select_type)?;
        Ok(self)
    }

    pub(crate) fn clip_streams(&mut self, clip: &ClipRange) {
        for stream in &mut self.streams {
            let mut start_idx = 0;
            let mut end_idx = stream.segments.len();
            let mut cursor = 0.0_f32;

            for (i, segment) in stream.segments.iter().enumerate() {
                let seg_start = cursor;
                let seg_end = cursor + segment.duration;
                cursor = seg_end;

                if seg_end <= clip.start {
                    start_idx = i + 1;
                    continue;
                }

                if let Some(end) = clip.end
                    && seg_start >= end
                {
                    end_idx = i;
                    break;
                }
            }

            let first_map = stream.segments.first().and_then(|s| s.map.clone());
            let first_key = stream.segments.first().and_then(|s| s.key.clone());
            stream.segments.truncate(end_idx);
            stream.segments.drain(..start_idx);

            if let Some(first) = stream.segments.first_mut() {
                if first.map.is_none() {
                    first.map = first_map;
                }
                if first.key.is_none() {
                    first.key = first_key;
                }
            }
        }
    }

    pub(crate) async fn metadata(
        &self,
        config: &PlaylistDownloadConfig,
    ) -> Result<Vec<StreamMetadata>> {
        let mut metadata = Vec::with_capacity(self.streams.len());

        for (i, stream) in self.streams.iter().enumerate() {
            let mut default_kid = stream.default_kid();
            let mut pssh = Vec::new();

            if let Some(bytes) = stream.fetch_init(config).await? {
                if let Some(key_id) = TencBox::from_init(&bytes)?.map(|x| x.default_kid_hex())
                    && key_id != "00000000000000000000000000000000"
                {
                    default_kid = Some(key_id);
                }

                for data in PsshBox::from_init(&bytes)?.boxes {
                    pssh.push(data.as_base64());
                }
            }

            metadata.push(StreamMetadata {
                bandwidth: stream.bandwidth,
                channels: stream.channels,
                codecs: stream.codecs.clone(),
                default_kid,
                encryption_type: stream
                    .segments
                    .first()
                    .and_then(|s| s.key.as_ref().map(|k| k.method.clone()))
                    .unwrap_or_default(),
                frame_rate: stream.frame_rate,
                index: i + 1,
                language: stream.language.clone(),
                media_type: stream.media_type.clone(),
                playlist_type: stream.playlist_type.clone(),
                pssh,
                resolution: stream.resolution,
            });
        }

        Ok(metadata)
    }
}

pub struct ClipRange {
    start: f32,
    end: Option<f32>,
}

impl ClipRange {
    pub fn new(s: &str) -> Result<Self> {
        let (start, end) = if let Some((a, b)) = s.split_once('-') {
            let (Some(start), Some(end)) = (Self::parse(a), Self::parse(b)) else {
                bail!("Clip range ({}) is invalid.", s);
            };

            if start >= end {
                bail!("Clip range start ({a}) must be before end ({b}).");
            }

            (start, Some(end))
        } else {
            let Some(start) = Self::parse(s) else {
                bail!("Clip range ({}) is invalid.", s);
            };
            (start, None)
        };

        Ok(Self { start, end })
    }

    fn parse(s: &str) -> Option<f32> {
        let parts = s.trim().split(':').collect::<Vec<_>>();

        match parts.len() {
            1 => parts[0].parse::<f32>().ok(),
            2 => {
                let ss = parts[1].parse::<f32>().ok()?;
                let mm = parts[0].parse::<f32>().ok()?;
                Some(mm.mul_add(60.0, ss))
            }
            3 => {
                let hh = parts[0].parse::<f32>().ok()?;
                let mm = parts[1].parse::<f32>().ok()?;
                let ss = parts[2].parse::<f32>().ok()?;
                Some(mm.mul_add(60.0, hh * 3600.0) + ss)
            }
            _ => None,
        }
    }
}
