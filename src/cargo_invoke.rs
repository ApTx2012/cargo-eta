use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

use anyhow::{Context, Result};

use crate::eta::EtaCalculator;
use crate::event::RawMessage;
use crate::model::{TimingModel, UnitKey};
use crate::render::Renderer;
use crate::unit_graph::{package_name, package_version, UnitGraph};

/// Result of running one cargo invocation.
pub struct BuildOutcome {
    pub success: bool,
    /// Observations to fold into the model; empty on failure.
    pub observations: Vec<(UnitKey, f64)>,
    /// Final progress snapshot, for the summary line.
    pub final_snapshot: crate::eta::ProgressSnapshot,
}

/// Options extracted from the command line and passed down to cargo.
pub struct Invocation<'a> {
    /// The cargo subcommand: build / check / test / clippy.
    pub subcommand: &'a str,
    /// Pass-through flags after the subcommand (e.g. --release, --target ...).
    pub passthrough: &'a [String],
}

impl Invocation<'_> {
    /// Detect the effective profile name for model keying.
    ///
    /// We look at explicit flags rather than Cargo's reported opt_level so
    /// the key is stable even before any artifact arrives.
    pub fn profile_name(&self) -> String {
        if self.passthrough.iter().any(|a| a == "--release") {
            "release".to_string()
        } else if let Some(pos) = self.passthrough.iter().position(|a| a == "--profile") {
            self.passthrough
                .get(pos + 1)
                .cloned()
                .unwrap_or_else(|| "dev".to_string())
        } else {
            "dev".to_string()
        }
    }

    /// Target triple if the user passed one, else the host default marker.
    pub fn target_triple(&self) -> String {
        if let Some(pos) = self.passthrough.iter().position(|a| a == "--target") {
            if let Some(t) = self.passthrough.get(pos + 1) {
                return t.clone();
            }
        }
        "host".to_string()
    }
}

/// Try to obtain the total unit count from `cargo --unit-graph`.
///
/// This subcommand is nightly-only (`-Z unstable-options`). On stable it
/// fails; callers must fall back to dynamic discovery. Returns `None`
/// rather than an error so the fallback is seamless.
pub fn try_unit_graph(inv: &Invocation) -> Option<UnitGraph> {
    let mut cmd = Command::new("cargo");
    cmd.arg(inv.subcommand)
        .arg("--unit-graph")
        .arg("--message-format=json")
        .arg("-Z")
        .arg("unstable-options");
    for a in inv.passthrough {
        cmd.arg(a);
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    // The unit graph is the last JSON object on stdout; diagnostics may
    // precede it. Parse lines in reverse and take the first valid graph.
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines().rev() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(graph) = serde_json::from_str::<UnitGraph>(line) {
            if !graph.units.is_empty() {
                return Some(graph);
            }
        }
    }
    None
}

/// Run cargo, streaming its JSON messages and updating `calc`/`renderer`.
///
/// On success the caller receives the observations for the model. On failure
/// the observations list is empty: a failed build's timings are not
/// representative and must not pollute the model.
pub fn run(
    inv: &Invocation,
    calc: &mut EtaCalculator,
    renderer: &mut Renderer,
    model: &TimingModel,
    update_every: usize,
) -> Result<BuildOutcome> {
    let mut cmd = Command::new("cargo");
    cmd.arg(inv.subcommand)
        .arg("--message-format=json")
        .stdout(Stdio::piped())
        // Let cargo's own stderr (the classic "Compiling foo" lines) through,
        // so users still see raw output if they scroll back.
        .stderr(Stdio::inherit());
    for a in inv.passthrough {
        cmd.arg(a);
    }

    let mut child = cmd
        .spawn()
        .with_context(|| format!("failed to spawn `cargo {}`", inv.subcommand))?;

    let stdout = child.stdout.take().context("cargo stdout was not piped")?;
    let reader = BufReader::new(stdout);

    let profile = inv.profile_name();
    let triple = inv.target_triple();

    let mut success = false;
    let mut tick: usize = 0;

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let msg: RawMessage = match serde_json::from_str(line) {
            Ok(m) => m,
            // Non-JSON stray output (rare) is ignored.
            Err(_) => continue,
        };

        match msg.reason.as_str() {
            "compiler-artifact" | "build-script-executed" => {
                let Some(pkg_id) = &msg.package_id else { continue };
                let name = msg
                    .crate_name()
                    .map(str::to_string)
                    .or_else(|| package_name(pkg_id))
                    .unwrap_or_else(|| pkg_id.clone());
                let version = package_version(pkg_id).unwrap_or_default();
                let key = UnitKey {
                    crate_name: name,
                    version,
                    profile: profile.clone(),
                    target_triple: triple.clone(),
                    proc_macro: msg.is_proc_macro(),
                };

                // Cargo emits the artifact only once the unit is done; the
                // "start" is implicit. We mark start+finish together, but
                // preserve the ability to track a running unit by looking
                // it up first.
                calc.start_unit(&key, model);
                let fresh = msg.fresh.unwrap_or(false);
                calc.finish_unit(&key, fresh);

                tick += 1;
                if tick % update_every == 0 {
                    let snap = calc.snapshot();
                    renderer.update(&snap);
                }
            }
            "build-finished" => {
                success = msg.success.unwrap_or(false);
            }
            _ => {}
        }
    }

    let status = child.wait().context("failed to wait for cargo")?;
    // `build-finished` is authoritative, but guard against a missing event.
    success = success && status.success();

    let snap = calc.snapshot();
    renderer.update(&snap);
    renderer.finish(&snap, success);
    renderer.finish_json(&snap, success);

    let observations = if success {
        // Only take observations if we still own the calculator; the caller
        // passed &mut, so we drain by re-reading. To keep ownership simple,
        // the calculator exposes a drain method.
        calc.drain_observations()
    } else {
        Vec::new()
    };

    Ok(BuildOutcome {
        success,
        observations,
        final_snapshot: snap,
    })
}