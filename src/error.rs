use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Prompt(#[from] dialoguer::Error),

    #[error("{0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("{0}")]
    TomlSerialize(#[from] toml::ser::Error),

    #[error("{0}")]
    TomlDeserialize(#[from] toml::de::Error),

    #[error("{0}")]
    Json(#[from] serde_json::Error),

    /// A user-cancelled interactive prompt (e.g. Esc on a Select).
    #[error("Selection cancelled")]
    Cancelled,

    /// A value was expected but not found (missing repo, worktree, etc.).
    #[error("{0}")]
    NotFound(String),

    /// A command or operation failed with a descriptive message.
    #[error("{0}")]
    Failed(String),
}

impl Error {
    /// Convenience constructor — prefer the typed variants when possible.
    pub fn failed(msg: impl Into<String>) -> Self {
        Self::Failed(msg.into())
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }
}
