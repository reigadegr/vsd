/// A specialized [`Result`] type for operations that can fail in `vsd-mp4`.
pub type Result<T> = std::result::Result<T, Error>;

/// The error type returned by functions in `vsd-mp4`.
#[derive(Debug)]
pub enum Error {
    /// The decryption key size is invalid (expected 16 bytes).
    InvalidKeySize(usize),
    /// An I/O error occurred during reading or writing.
    Io(std::io::Error),
    /// A generic or custom error accompanied by a message.
    Other(String),
}

impl std::error::Error for Error {}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidKeySize(x) => {
                write!(f, "invalid key size: expected 16 bytes got {x} bytes.")
            }
            Self::Io(x) => write!(f, "i/o error: {x}"),
            Self::Other(x) => write!(f, "{x}"),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<std::string::FromUtf8Error> for Error {
    fn from(e: std::string::FromUtf8Error) -> Self {
        Self::Other(e.to_string())
    }
}

impl From<std::string::FromUtf16Error> for Error {
    fn from(e: std::string::FromUtf16Error) -> Self {
        Self::Other(e.to_string())
    }
}

#[cfg(any(feature = "decrypt-cenc", feature = "pssh"))]
impl From<hex::FromHexError> for Error {
    fn from(e: hex::FromHexError) -> Self {
        Self::Other(format!("hex decode error: {e}"))
    }
}

#[cfg(feature = "pssh")]
impl From<prost::DecodeError> for Error {
    fn from(e: prost::DecodeError) -> Self {
        Self::Other(format!("protobuf decode error: {e}"))
    }
}

#[cfg(feature = "pssh")]
impl From<quick_xml::de::DeError> for Error {
    fn from(e: quick_xml::de::DeError) -> Self {
        Self::Other(format!("xml parse error: {e}"))
    }
}

/// Early-returns with an [`Error::Other`] variant.
///
/// This macro accepts the same format arguments as [`format!`].
#[macro_export]
macro_rules! bail {
    ($($arg:tt)*) => {
        return Err($crate::Error::Other(format!($($arg)*)))
    };
}
