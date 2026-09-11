use crate::{
    dash::{format_locator, parse_channels, parse_frame_rate},
    playlist::{MasterPlaylist, MediaPlaylist, MediaType, PlaylistType},
    utils,
};
use dash_mpd::MPD;
use reqwest::Url;

pub fn parse_as_master(base_url: &Url, mpd: &MPD) -> MasterPlaylist {
    let mut playlist = MasterPlaylist {
        playlist_type: PlaylistType::Dash,
        streams: Vec::new(),
        uri: base_url.to_string(),
    };

    let Some(period) = mpd.periods.first() else {
        return playlist;
    };

    for (adaptation_index, adaptation_set) in period.adaptations.iter().enumerate() {
        for (representation_index, representation) in
            adaptation_set.representations.iter().enumerate()
        {
            let locator = format_locator(adaptation_index, representation_index);

            let codecs = representation
                .codecs
                .clone()
                .or(adaptation_set.codecs.clone());

            let mime_type = representation
                .mimeType
                .clone()
                .or(adaptation_set.mimeType.clone())
                .or(representation.contentType.clone())
                .or(adaptation_set.contentType.clone());

            let mut media_type = if let Some(mime_type) = &mime_type {
                match mime_type.as_str() {
                    "application/ttml+xml" | "application/x-sami" => MediaType::Subtitles,
                    x if x.starts_with("audio") => MediaType::Audio,
                    x if x.starts_with("text") => MediaType::Subtitles,
                    x if x.starts_with("video") => MediaType::Video,
                    _ => MediaType::Undefined,
                }
            } else {
                MediaType::Undefined
            };

            if media_type == MediaType::Undefined
                && let Some(codecs) = &codecs
            {
                media_type = match codecs.as_str() {
                    "wvtt" | "stpp" => MediaType::Subtitles,
                    x if x.starts_with("stpp.") => MediaType::Subtitles,
                    _ => media_type,
                };
            }

            playlist.streams.push(MediaPlaylist {
                bandwidth: representation.bandwidth,
                channels: parse_channels(&representation.AudioChannelConfiguration)
                    .or_else(|| parse_channels(&adaptation_set.AudioChannelConfiguration)),
                codecs,
                extension: mime_type
                    .as_ref()
                    .and_then(|x| x.split_once('/').map(|(_, y)| y.to_owned())),
                frame_rate: representation
                    .frameRate
                    .as_ref()
                    .and_then(|x| parse_frame_rate(x))
                    .or_else(|| {
                        adaptation_set
                            .frameRate
                            .as_ref()
                            .and_then(|x| parse_frame_rate(x))
                    }),
                id: utils::gen_id(base_url.as_str(), &locator),
                i_frame: false,
                language: adaptation_set.lang.clone(),
                live: mpd.mpdtype.as_ref().is_some_and(|x| x == "dynamic"),
                media_sequence: 0,
                media_type,
                playlist_type: PlaylistType::Dash,
                resolution: representation.width.zip(representation.height),
                segments: Vec::new(),
                uri: locator,
            });
        }
    }

    playlist
}
