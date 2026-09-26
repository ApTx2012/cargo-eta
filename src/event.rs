use serde::Deserialize;

/// One line of `cargo --message-format=json` output.
///
/// Cargo tags every message with a `reason` field. We deserialize field by
/// field rather than into a closed enum, so unknown message types simply
/// leave the optional fields as `None` and are ignored by the caller.
#[derive(Debug, Clone, Deserialize)]
pub struct RawMessage {
    pub reason: String,

    /// Present for `compiler-artifact` and `build-script-executed`.
    #[serde(default)]
    pub package_id: Option<String>,

    /// Present for `compiler-artifact`. Cargo calls this `target`.
    #[serde(default)]
    pub target: Option<ArtifactTarget>,

    /// Present for `compiler-artifact`. `true` if this was a fresh (cached) unit.
    #[serde(default)]
    pub fresh: Option<bool>,

    /// Present for `compiler-artifact`: build profile info.
    #[serde(default)]
    pub profile: Option<ArtifactProfile>,

    /// Present for `build-finished`.
    #[serde(default)]
    pub success: Option<bool>,
}

impl RawMessage {
    /// Best-effort crate name from a `compiler-artifact` message.
    pub fn crate_name(&self) -> Option<&str> {
        self.target.as_ref().and_then(|t| t.name.as_deref())
    }

    /// Whether this artifact is a proc-macro (affects compile-time modelling).
    pub fn is_proc_macro(&self) -> bool {
        self.target
            .as_ref()
            .and_then(|t| t.kind.as_ref())
            .map(|kinds| kinds.iter().any(|k| k == "proc-macro"))
            .unwrap_or(false)
    }

    /// Whether this artifact is a test binary (kind contains "test").
    pub fn is_test(&self) -> bool {
        self.target
            .as_ref()
            .and_then(|t| t.kind.as_ref())
            .map(|kinds| kinds.iter().any(|k| k == "test"))
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ArtifactTarget {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub kind: Option<Vec<String>>,
    #[serde(default)]
    pub crate_types: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ArtifactProfile {
    #[serde(default)]
    pub opt_level: Option<String>,
    #[serde(default)]
    pub debug_assertions: Option<bool>,
}