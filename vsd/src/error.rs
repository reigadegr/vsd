//! Error types and propagation macros for `vsd`.

use reqwest::StatusCode;

/// A specialized [`Result`] type for operations in this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Represents errors that can occur during playlist fetching, parsing, downloading, or merging.
#[derive(Debug)]
pub enum Error {
    /// Failed to parse a Netscape cookie.
    CookieParse(crate::cookie::ParseError),
    /// Failed to parse or resolve MPEG-DASH addressing parameters.
    DashParse(String),
    /// The download process was interrupted or cancelled.
    DownloadInterrupted,
    /// An error occurred while executing the ffmpeg binary.
    FfmpegFailed {
        /// The exit status code returned by ffmpeg.
        code: i32,
        /// The error message or stdout/stderr output.
        message: String,
    },
    /// Failed to parse a format selection expression.
    FormatParse(String),
    /// A decryption key was required but could not be resolved.
    ///
    /// Stores the default key ID (KID) in hexadecimal.
    MissingKey(String),
    /// The media playlist contains no segments to download.
    MissingSegments,
    /// An error occurred in the underlying `vsd-mp4` parsing crate.
    Mp4Parse(vsd_mp4::Error),
    /// A generic or format-specific error message.
    Other(String),
    /// An HTTP request failed.
    RequestFailed {
        /// The request URL.
        url: String,
        /// The HTTP response status code.
        status: StatusCode,
        /// The response body context or error description.
        body: String,
    },
    /// The playlist uses an unsupported encryption scheme.
    UnsupportedEncryption(String),
}

impl std::error::Error for Error {}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CookieParse(x) => write!(f, "Failed to parse netscape cookie: {x}."),
            Self::DashParse(x) => write!(f, "Failed to resolve dash addressing: {x}"),
            Self::DownloadInterrupted => write!(f, "Download interrupted due to ctrl+c."),
            Self::FfmpegFailed { code, message } => {
                write!(f, "Failed to execute ffmpeg ({code}): {message}")
            }
            Self::FormatParse(x) => write!(f, "Failed to parse format expr: {x}."),
            Self::MissingKey(x) => write!(f, "Missing decryption key for {x}."),
            Self::MissingSegments => write!(f, "Stream contains no segments."),
            Self::Mp4Parse(x) => write!(f, "vsd-mp4: {x}"),
            Self::Other(x) => write!(f, "{x}"),
            Self::RequestFailed { url, status, body } => {
                write!(f, "Failed to request {url} ({status}): {body}")
            }
            Self::UnsupportedEncryption(x) => write!(
                f,
                "Unsupported encryption method: {x}. Use --no-decrypt flag to download encrypted streams."
            ),
        }
    }
}

impl From<crate::cookie::ParseError> for Error {
    fn from(e: crate::cookie::ParseError) -> Self {
        Self::CookieParse(e)
    }
}

impl From<vsd_mp4::Error> for Error {
    fn from(e: vsd_mp4::Error) -> Self {
        Self::Mp4Parse(e)
    }
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        Self::RequestFailed {
            url: e.url().map_or("unknown", reqwest::Url::as_str).to_owned(),
            status: e.status().unwrap_or_default(),
            body: e.to_string(),
        }
    }
}

/// Helper macro to generate `From` conversions for external error types into `Error::Other`.
macro_rules! impl_from_other {
    ($($t:ty),*) => {
        $(
            impl From<$t> for Error {
                fn from(e: $t) -> Self {
                    Self::Other(e.to_string())
                }
            }
        )*
    };
}

impl_from_other!(
    String,
    std::array::TryFromSliceError,
    std::io::Error,
    std::num::ParseIntError,
    std::string::FromUtf8Error,
    base64::DecodeError,
    serde_json::Error,
    requestty::ErrorKind,
    reqwest::header::InvalidHeaderName,
    reqwest::header::InvalidHeaderValue,
    reqwest::header::ToStrError,
    tokio::task::JoinError,
    url::ParseError
);

#[cfg(feature = "capture")]
impl_from_other!(chromiumoxide::error::CdpError);

#[cfg(feature = "license")]
impl_from_other!(playready::Error, widevine::Error);

/// Early-returns with an [`Error::Other`]. Accepts formatted message parameters.
#[macro_export]
macro_rules! bail {
    ($msg:literal $(,)?) => {
        return Err($crate::error::Error::Other($msg.into()))
    };
    ($fmt:expr, $($arg:tt)*) => {
        return Err($crate::error::Error::Other(format!($fmt, $($arg)*)))
    };
}
