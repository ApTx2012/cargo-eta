use serde::Deserialize;

/// Minimal view of Cargo's `--unit-graph` JSON output.
///
/// The full schema is documented in the Cargo book (unstable, nightly only).
/// We only need the list of build units and enough info to key them.
#[derive(Debug, Clone, Deserialize)]
pub struct UnitGraph {
    pub version: u32,
    #[serde(default)]
    pub units: Vec<Unit>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Unit {
    pub pkg_id: String,
    #[serde(default)]
    pub target: Option<UnitTarget>,
    #[serde(default)]
    pub profile: Option<UnitProfile>,
    #[serde(default)]
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UnitTarget {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub kind: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UnitProfile {
    #[serde(default)]
    pub opt_level: Option<String>,
}

impl UnitGraph {
    /// Total number of build units Cargo plans to build.
    pub fn total_units(&self) -> usize {
        self.units.len()
    }

    /// Turn the graph's units into model keys.
    ///
    /// A single crate can appear several times (lib + build script + test
    /// harness), each a separate compilation unit and thus a separate
    /// timing observation.
    pub fn planned_keys(
        &self,
        profile: &str,
        target_triple: &str,
    ) -> Vec<crate::model::UnitKey> {
        self.units
            .iter()
            .map(|u| {
                let name = u
                    .target
                    .as_ref()
                    .and_then(|t| t.name.clone())
                    .or_else(|| package_name(&u.pkg_id))
                    .unwrap_or_else(|| u.pkg_id.clone());
                let version = package_version(&u.pkg_id).unwrap_or_default();
                let proc_macro = u
                    .target
                    .as_ref()
                    .and_then(|t| t.kind.as_ref())
                    .map(|k| k.iter().any(|x| x == "proc-macro"))
                    .unwrap_or(false);
                crate::model::UnitKey {
                    crate_name: name,
                    version,
                    profile: profile.to_string(),
                    target_triple: target_triple.to_string(),
                    proc_macro,
                }
            })
            .collect()
    }
}

/// Pull the crate name out of a Cargo `package_id` string.
///
/// Package IDs look like:
///   `path+file:///D:/proj#my-crate@0.1.0`
///   `registry+https://github.com/rust-lang/crates.io-index#serde@1.0.0`
/// The name is the part after the last `#`, before the `@`.
pub fn package_name(pkg_id: &str) -> Option<String> {
    let after_hash = pkg_id.rsplit('#').next().unwrap_or(pkg_id);
    let name = after_hash.split('@').next().unwrap_or(after_hash);
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// Pull the version out of a Cargo `package_id` string (part after `@`).
pub fn package_version(pkg_id: &str) -> Option<String> {
    let after_hash = pkg_id.rsplit('#').next().unwrap_or(pkg_id);
    let mut parts = after_hash.splitn(2, '@');
    parts.next()?;
    parts.next().map(|s| s.to_string())
}