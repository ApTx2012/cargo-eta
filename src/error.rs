use std::path::PathBuf;

/// Top-level error type for cargo-eta.
///
/// Most failures bubble up as `anyhow::Error` at the call sites, but a few
/// cases deserve explicit context so the user gets a readable message
/// instead of a raw OS error.
#[derive(Debug)]
pub enum EtaError {
    /// The `cargo` executable could not be found or launched.
    CargoNotFound,
    /// Cargo exited with a non-zero status while we were streaming output.
    CargoFailed { code: Option<i32> },
    /// Reading or writing the persistent cache file failed.
    CacheIo { path: PathBuf, source: std::io::Error },
    /// The cache file exists but is not valid JSON.
    CacheCorrupt { path: PathBuf, source: serde_json::Error },
}

impl std::fmt::Display for EtaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EtaError::CargoNotFound => write!(f, "could not find or launch `cargo`"),
            EtaError::CargoFailed { code } => {
                write!(f, "cargo exited with status {}", code.map_or("signal".into(), |c| c.to_string()))
            }
            EtaError::CacheIo { path, source } => {
                write!(f, "failed to access cache at {}: {source}", path.display())
            }
            EtaError::CacheCorrupt { path, source } => {
                write!(f, "cache at {} is corrupt: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for EtaError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            EtaError::CacheIo { source, .. } => Some(source),
            EtaError::CacheCorrupt { source, .. } => Some(source),
            _ => None,
        }
    }
}