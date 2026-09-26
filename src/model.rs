use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Format version for the on-disk cache. Bump on incompatible schema changes.
const CACHE_VERSION: u32 = 1;

/// Weight of the newest sample in the exponential weighted moving average.
/// 0.3 means a single outlier moves the estimate by at most 30%.
const EWMA_ALPHA: f64 = 0.3;

/// Conservative fallback when we have never seen a crate before.
/// Intentionally on the slow side so the first build's ETA errs towards
/// "too long" rather than "too short".
pub const DEFAULT_CRATE_SECONDS: f64 = 1.5;

/// A single crate's persisted timing statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimingEntry {
    /// EWMA of observed wall-clock seconds for this unit.
    pub seconds: f64,
    /// How many samples contributed to `seconds`.
    pub samples: u64,
    /// Unix seconds of the last update, for future ageing/TTL logic.
    #[serde(default)]
    pub last_updated: u64,
}

/// The whole persistent model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimingModel {
    pub version: u32,
    /// Map from a composite key (see `UnitKey::as_key`) to its statistics.
    pub entries: HashMap<String, TimingEntry>,
}

impl Default for TimingModel {
    fn default() -> Self {
        Self {
            version: CACHE_VERSION,
            entries: HashMap::new(),
        }
    }
}

/// Everything that distinguishes one compilation unit from another for
/// timing. Changing the profile or target can change compile time by an
/// order of magnitude, so both are part of the key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UnitKey {
    pub crate_name: String,
    pub version: String,
    /// "dev" / "release" / other, derived from the invocation flags.
    pub profile: String,
    pub target_triple: String,
    pub proc_macro: bool,
}

impl UnitKey {
    /// Flatten into the string used as a JSON map key.
    pub fn as_key(&self) -> String {
        format!(
            "{}@{}|{}|{}|{}",
            self.crate_name,
            self.version,
            self.profile,
            self.target_triple,
            if self.proc_macro { "proc" } else { "lib" }
        )
    }
}

impl TimingModel {
    /// Load the model from disk, returning an empty one if the file is
    /// missing or unreadable. A corrupt file is treated as empty (and will
    /// be overwritten on the next save) so a bad cache never blocks a build.
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str::<TimingModel>(&text) {
                Ok(m) if m.version == CACHE_VERSION => m,
                Ok(_) => TimingModel::default(),
                Err(e) => {
                    eprintln!("cargo-eta: ignoring corrupt cache ({}): {e}", path.display());
                    TimingModel::default()
                }
            },
            Err(_) => TimingModel::default(),
        }
    }

    /// Persist the model. Errors are reported but never fatal: a failure to
    /// write the cache must not fail the user's build.
    pub fn save(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match serde_json::to_string_pretty(self) {
            Ok(text) => {
                if let Err(e) = std::fs::write(path, text) {
                    eprintln!("cargo-eta: could not write cache ({}): {e}", path.display());
                }
            }
            Err(e) => eprintln!("cargo-eta: could not serialise cache: {e}"),
        }
    }

    /// Expected duration for a unit, or the conservative default if unknown.
    pub fn expected_seconds(&self, key: &str) -> f64 {
        self.entries
            .get(key)
            .map(|e| e.seconds)
            .unwrap_or(DEFAULT_CRATE_SECONDS)
    }

    /// Whether we have any history for this unit.
    pub fn has_history(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }

    /// Fold a newly observed duration into the model using an EWMA.
    ///
    /// The first observation seeds the entry directly; subsequent ones blend
    /// with weight `EWMA_ALPHA`, so a single slow build only nudges the
    /// estimate.
    pub fn record(&mut self, key: &str, observed_seconds: f64) {
        if observed_seconds <= 0.0 || !observed_seconds.is_finite() {
            return;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        match self.entries.get_mut(key) {
            Some(entry) => {
                entry.seconds = EWMA_ALPHA * observed_seconds + (1.0 - EWMA_ALPHA) * entry.seconds;
                entry.samples += 1;
                entry.last_updated = now;
            }
            None => {
                self.entries.insert(
                    key.to_string(),
                    TimingEntry {
                        seconds: observed_seconds,
                        samples: 1,
                        last_updated: now,
                    },
                );
            }
        }
    }
}

/// Resolve `~/.cargo/eta-cache.json` without pulling in the `dirs` crate.
///
/// On Windows `HOME` is usually unset in cmd.exe, so `USERPROFILE` is the
/// fallback. `CARGO_HOME` wins over both when the user has customised it.
pub fn cache_path() -> PathBuf {
    if let Ok(cargo_home) = std::env::var("CARGO_HOME") {
        if !cargo_home.is_empty() {
            return PathBuf::from(cargo_home).join("eta-cache.json");
        }
    }

    let home = std::env::var("USERPROFILE")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("HOME").ok().filter(|s| !s.is_empty()));

    match home {
        Some(h) => PathBuf::from(h).join(".cargo").join("eta-cache.json"),
        None => PathBuf::from("eta-cache.json"),
    }
}