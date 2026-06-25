use thiserror::Error;

use crate::runtime::process::ExternalTool;

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

    #[error("Worktree has uncommitted changes. Use --force if you really want to remove it.")]
    DirtyWorktree,
}

impl Error {
    /// Convenience constructor — prefer the typed variants when possible.
    pub fn failed(msg: impl Into<String>) -> Self {
        Self::Failed(msg.into())
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }

    /// Build a user-facing error for a required external tool that is not on
    /// PATH. Includes the tool's install hint so the message is actionable.
    #[must_use]
    pub fn tool_missing(tool: ExternalTool) -> Self {
        Self::Failed(format!(
            "Required tool `{binary}` not found on PATH. Install with: {hint}",
            binary = tool.binary_name(),
            hint = tool.install_hint(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::Error;

    mod constructors {
        use super::*;

        #[test]
        fn failed_stores_message() {
            let err = Error::failed("something went wrong");
            assert_eq!(err.to_string(), "something went wrong");
        }

        #[test]
        fn not_found_stores_message() {
            let err = Error::not_found("repo missing");
            assert_eq!(err.to_string(), "repo missing");
        }

        #[test]
        fn failed_accepts_string_and_str() {
            let from_str = Error::failed("from &str");
            let from_string = Error::failed(String::from("from String"));

            assert_eq!(from_str.to_string(), "from &str");
            assert_eq!(from_string.to_string(), "from String");
        }
    }

    mod display {
        use super::*;

        #[test]
        fn cancelled_has_fixed_display() {
            let err = Error::Cancelled;
            assert_eq!(err.to_string(), "Selection cancelled");
        }

        #[test]
        fn dirty_worktree_mentions_uncommitted_changes_and_force() {
            let msg = Error::DirtyWorktree.to_string();

            assert!(
                msg.contains("uncommitted changes"),
                "message should mention uncommitted changes: {msg}"
            );
            assert!(
                msg.contains("--force"),
                "message should mention --force: {msg}"
            );
        }
    }

    mod from_conversions {
        use super::*;

        #[test]
        fn from_io_error_preserves_message() {
            let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
            let err = Error::from(io_err);
            assert!(err.to_string().contains("file not found"));
            assert!(matches!(err, Error::Io(_)));
        }

        #[test]
        fn from_json_error_is_json_variant() {
            let json_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
            let err = Error::from(json_err);
            assert!(matches!(err, Error::Json(_)));
        }
    }

    mod tool_missing {
        use super::*;
        use crate::runtime::process::ExternalTool;

        #[test]
        fn mentions_binary_name_and_install_hint() {
            let err = Error::tool_missing(ExternalTool::Git);
            let msg = err.to_string();
            assert!(msg.contains("`git`"), "message should quote binary: {msg}");
            assert!(
                msg.contains("not found on PATH"),
                "message should say not found: {msg}"
            );
            assert!(
                msg.contains("nix profile install nixpkgs#git"),
                "message should include install hint: {msg}"
            );
        }

        #[test]
        fn uses_tool_specific_nix_package() {
            let zellij_err = Error::tool_missing(ExternalTool::Zellij);
            assert!(zellij_err.to_string().contains("nixpkgs#zellij"));

            let opencode_err = Error::tool_missing(ExternalTool::Opencode);
            assert!(opencode_err.to_string().contains("nixpkgs#opencode"));
        }

        #[test]
        fn cargo_hint_mentions_rustup() {
            let err = Error::tool_missing(ExternalTool::Cargo);
            assert!(err.to_string().contains("rustup"));
        }

        #[test]
        fn nix_hint_points_at_nixos_download() {
            let err = Error::tool_missing(ExternalTool::Nix);
            assert!(err.to_string().contains("nixos.org/download"));
        }
    }
}
