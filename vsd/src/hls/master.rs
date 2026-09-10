use crate::{playlist, utils};
use reqwest::Url;

pub fn parse_as_master(base_url: &Url, m3u8: &m3u8_rs::MasterPlaylist) -> playlist::MasterPlaylist {
    let mut streams = Vec::new();

    for stream in &m3u8.variants {
        streams.push(playlist::MediaPlaylist {
            bandwidth: Some(stream.bandwidth),
            codecs: stream.codecs.clone(),
            extension: Some("ts".to_owned()),
            frame_rate: stream.frame_rate.map(|x| x as f32),
            id: utils::gen_id(base_url.as_str(), &stream.uri),
            i_frame: stream.is_i_frame,
            media_type: playlist::MediaType::Video,
            playlist_type: playlist::PlaylistType::Hls,
            resolution: stream.resolution.map(|r| (r.width, r.height)),
            uri: stream.uri.clone(),
            ..Default::default()
        });
    }

    for alt in &m3u8.alternatives {
        let Some(uri) = &alt.uri else {
            continue;
        };

        streams.push(playlist::MediaPlaylist {
            bandwidth: alt.other_attributes.as_ref().and_then(|x| {
                x.get("BANDWIDTH")
                    .and_then(|x| x.as_str().parse::<u64>().ok())
            }),
            channels: alt.channels.as_ref().and_then(|x| x.parse::<f32>().ok()),
            codecs: alt
                .other_attributes
                .as_ref()
                .and_then(|x| x.get("CODECS").map(|x| x.as_str().to_owned())),
            extension: match alt.media_type {
                m3u8_rs::AlternativeMediaType::ClosedCaptions
                | m3u8_rs::AlternativeMediaType::Subtitles => Some("vtt".to_owned()),
                _ => Some("ts".to_owned()),
            },
            id: utils::gen_id(base_url.as_str(), uri),
            language: alt.language.clone().or(alt.assoc_language.clone()),
            media_type: match alt.media_type {
                m3u8_rs::AlternativeMediaType::Audio => playlist::MediaType::Audio,
                m3u8_rs::AlternativeMediaType::ClosedCaptions
                | m3u8_rs::AlternativeMediaType::Subtitles => playlist::MediaType::Subtitles,
                m3u8_rs::AlternativeMediaType::Other(_) => playlist::MediaType::Undefined,
                m3u8_rs::AlternativeMediaType::Video => playlist::MediaType::Video,
            },
            playlist_type: playlist::PlaylistType::Hls,
            uri: uri.to_owned(),
            ..Default::default()
        });
    }

    playlist::MasterPlaylist {
        playlist_type: playlist::PlaylistType::Hls,
        uri: base_url.to_string(),
        streams,
    }
}
