use failure::Fail;
use std::io;

#[derive(Fail, Debug)]
pub enum KvsError {
    #[fail(display = "{} is not a valid version.", _0)]
    InvalidVersion(u32),

    #[fail(display = "IO error: {}", error)]
    IoError { error: io::Error },

    #[fail(display = "Serialization error: {} ", error)]
    SerializeError { error: serde_json::Error },

    #[fail(
        display = "Deserialization failed for line: {} with error: {}",
        line, error
    )]
    DeserializedError {
        line: String,
        error: serde_json::Error,
    },

    #[fail(display = "Key doesn't exist.")]
    KeyNotFound,

    #[fail(display = "An unknown error has occurred.")]
    UnknownError,
}

/// Result type for kvs.
pub type Result<T> = std::result::Result<T, KvsError>;
