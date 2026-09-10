use serde::{Deserialize, Serialize};

/// Represents the top-level master playlist containing one or more media streams.
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct MasterPlaylist {
    /// The type of the playlist (e.g., HLS or DASH).
    pub playlist_type: PlaylistType,
    /// The absolute or base URI of the master playlist.
    pub uri: String,
    /// The media streams (audio, video, subtitles) defined in the master playlist.
    pub streams: Vec<MediaPlaylist>,
}

/// Represents an individual media stream (audio, video, or subtitles) with associated metadata.
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct MediaPlaylist {
    /// The bandwidth requirement of the stream in bits per second.
    pub bandwidth: Option<u64>,
    /// The number of audio channels (e.g., 2.0, 5.1).
    pub channels: Option<f32>,
    /// The codecs used in the stream (e.g., `avc1.640028,mp4a.40.2`).
    pub codecs: Option<String>,
    /// The preferred file extension for downloading/writing segments of this stream.
    pub extension: Option<String>,
    /// The frame rate of the video stream in frames per second.
    pub frame_rate: Option<f32>,
    /// A unique identifier for the stream.
    pub id: String,
    /// Indicates whether the playlist is an I-frame only playlist.
    pub i_frame: bool,
    /// The language tag of the stream (e.g., `en`, `es`).
    pub language: Option<String>,
    /// Indicates whether the stream is live or linear media (VOD otherwise).
    pub live: bool,
    /// The sequence number of the first segment in the playlist.
    pub media_sequence: u64,
    /// The type of media (video, audio, subtitles, etc.) in the stream.
    pub media_type: MediaType,
    /// The type of the playlist format (HLS or DASH).
    pub playlist_type: PlaylistType,
    /// The video resolution width and height, if applicable.
    pub resolution: Option<(u64, u64)>,
    /// The list of sequential segments comprising the stream.
    pub segments: Vec<Segment>,
    /// The URI of the individual media playlist.
    pub uri: String,
}

/// Represents a single media segment belonging to a stream.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Segment {
    /// The duration of the segment in seconds.
    pub duration: f32,
    /// Optional decryption key information for the segment.
    pub key: Option<Key>,
    /// Optional initialization segment (map/init) information.
    pub map: Option<Map>,
    /// Optional byte range of the segment within the resource.
    pub range: Option<Range>,
    /// The URI of the segment resource.
    pub uri: String,
}

/// Represents key information used to decrypt a segment.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Key {
    /// The default key ID (KID) associated with the encryption key.
    pub default_kid: Option<String>,
    /// The initialization vector (IV) for decryption.
    pub iv: Option<String>,
    /// The encryption method used.
    pub method: KeyMethod,
    /// The URI where the key can be fetched.
    pub uri: Option<String>,
}

/// Represents initialization segment information (typically for fMP4 streams).
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Map {
    /// Optional byte range of the initialization segment.
    pub range: Option<Range>,
    /// The URI of the initialization resource.
    pub uri: String,
}

/// Represents a contiguous byte range (start, end) inclusive.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Range(pub u64, pub u64);

/// The type of media content.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub enum MediaType {
    /// Video track stream.
    Video,
    /// Audio track stream.
    Audio,
    /// Subtitle track stream.
    Subtitles,
    /// Unknown or undefined media type.
    #[default]
    Undefined,
}

/// The streaming protocol/format type.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub enum PlaylistType {
    /// Dynamic Adaptive Streaming over HTTP (MPEG-DASH).
    Dash,
    /// HTTP Live Streaming (HLS).
    #[default]
    Hls,
}

/// The encryption/decryption method applied to the segments.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub enum KeyMethod {
    /// Envelope AES-128 encryption.
    Aes128,
    /// Common Encryption (CENC, typically for MPEG-DASH and fMP4 HLS).
    Cenc,
    /// Unencrypted stream.
    #[default]
    None,
    /// An unsupported or custom encryption method.
    Other(String),
    /// Sample AES-128 encryption.
    SampleAes,
}

/// Metadata describing a selected stream for serialization/JSON listing.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct StreamMetadata {
    /// The bandwidth requirements in bps.
    pub bandwidth: Option<u64>,
    /// The number of audio channels.
    pub channels: Option<f32>,
    /// The codecs used.
    pub codecs: Option<String>,
    /// The default key ID (KID) if encrypted.
    pub default_kid: Option<String>,
    /// The encryption method used.
    pub encryption_type: KeyMethod,
    /// The frame rate of the video.
    pub frame_rate: Option<f32>,
    /// The relative index of the stream.
    pub index: usize,
    /// The language tag.
    pub language: Option<String>,
    /// The media type (video, audio, etc.).
    pub media_type: MediaType,
    /// The playlist format type (HLS or DASH).
    pub playlist_type: PlaylistType,
    /// A list of DRM PSSH strings associated with the stream.
    pub pssh: Vec<String>,
    /// The video resolution width and height.
    pub resolution: Option<(u64, u64)>,
}
