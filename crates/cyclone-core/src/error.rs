use thiserror::Error;

pub type Result<T> = std::result::Result<T, CycloneError>;

#[derive(Debug, Error)]
pub enum CycloneError {
    #[error("profile not found at {0}; run `cyclone init` to create one")]
    ProfileMissing(String),

    #[error("image not found at {0}")]
    ImageNotFound(String),

    #[error("unsupported platform: {0}")]
    UnsupportedPlatform(String),

    #[error("encryption error: {0}")]
    Crypto(String),

    #[error("manifest validation failed: {0}")]
    InvalidManifest(String),

    #[error("{0}")]
    User(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Toml(#[from] toml::de::Error),

    #[error(transparent)]
    TomlSer(#[from] toml::ser::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
