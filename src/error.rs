#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to spawn child process: {0}")]
    Spawn(#[source] std::io::Error),

    #[error("BRP request `{method}` failed: {message}")]
    Brp { method: String, message: String },

    #[error("BRP watch/SSE response is not supported by v0.1: `{0}`")]
    UnsupportedWatch(String),

    #[error("selector `{0}` was not found")]
    SelectorNotFound(String),

    #[error("selector `{0}` matched more than one entity")]
    AmbiguousSelector(String),

    /// Timed-out operation. For startup, `operation` may include stdout/stderr snapshots.
    #[error("operation `{operation}` timed out after {timeout:?}")]
    Timeout {
        operation: String,
        timeout: std::time::Duration,
    },

    #[error("child process exited unexpectedly with status {0}")]
    ChildExited(std::process::ExitStatus),

    #[error("artifact operation failed: {0}")]
    Artifact(String),

    #[error("invalid E2E configuration: {0}")]
    Configuration(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub(crate) fn artifact_io(context: &str, error: std::io::Error) -> Self {
        Self::Artifact(format!("{context}: {error}"))
    }
}
