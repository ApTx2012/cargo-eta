//! Shim mode: pretend to be `cargo` so that tools which invoke cargo via
//! PATH (notably the Tauri CLI) can be intercepted.
//!
//! Stage 1 scope: log the invocation and forward everything verbatim to the
//! real cargo. This proves the interception path works without changing the
//! build's behaviour. Wiring the forwarded `build`/`check`/... into the ETA
//! pipeline comes later.
//!
//! RECURSION HAZARD: this binary is named `cargo.exe` and lives on PATH
//! ahead of the real one. If we resolved the forward target by name we would
//! re-enter ourselves forever. We therefore resolve the real cargo to an
//! absolute path, skipping any candidate that is this very executable.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

/// Name of the log file written on every intercepted call.
const LOG_NAME: &str = "cargo-eta-shim.log";

/// True if the current executable looks like it was invoked as `cargo`.
///
/// We look at the file stem of `current_exe()` rather than `argv[0]`,
/// because argv[0] can be anything the caller chose while the actual binary
/// on disk is what determines whether we are shadowing cargo.
pub fn invoked_as_cargo() -> bool {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return false,
    };
    stem_is_cargo(&exe)
}

fn stem_is_cargo(path: &Path) -> bool {
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("cargo"))
        .unwrap_or(false)
}

/// Run in shim mode: log, then forward to the real cargo.
///
/// Returns the exit code we should propagate.
pub fn run_shim() -> Result<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let started = std::time::Instant::now();

    let real = match find_real_cargo() {
        Some(p) => p,
        None => {
            // Record the failed lookup too, so a misconfigured PATH is
            // visible in the log rather than only on stderr.
            log_invocation(&args, started.elapsed().as_secs_f64(), None);
            bail!(
                "shim could not locate the real cargo executable on PATH \
                 (is cargo-eta shadowing cargo without a real cargo installed?)"
            );
        }
    };

    // Forward verbatim, inheriting all three standard streams so the user
    // sees exactly what cargo would have printed.
    let status = Command::new(&real)
        .args(&args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("failed to launch real cargo at {}", real.display()))?;

    let code = status.code().unwrap_or(1);
    log_invocation(&args, started.elapsed().as_secs_f64(), Some(code));
    Ok(code)
}

/// Append one line describing a completed invocation to the shim log.
///
/// Format:
///   [<unix-secs>] dur=<secs>s code=<code|-> :: cargo <args...>
///
/// `code` is `None` when the real cargo could not be located. Failures to
/// write are swallowed: logging must never break the user's build.
fn log_invocation(args: &[String], duration_secs: f64, exit_code: Option<i32>) {
    let code = exit_code
        .map(|c| c.to_string())
        .unwrap_or_else(|| "-".to_string());
    let line = format!(
        "[{}] dur={:.3}s code={} :: cargo {}\n",
        timestamp(),
        duration_secs,
        code,
        args.join(" ")
    );
    let path = log_path();
    // Best-effort; ignore any error.
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| {
            use std::io::Write;
            f.write_all(line.as_bytes())
        });
}

/// Where to write the shim log. `%TEMP%` on Windows, `/tmp` elsewhere,
/// falling back to the current directory.
fn log_path() -> PathBuf {
    if let Ok(tmp) = std::env::var("TEMP").or_else(|_| std::env::var("TMP")) {
        if !tmp.is_empty() {
            return PathBuf::from(tmp).join(LOG_NAME);
        }
    }
    std::env::temp_dir().join(LOG_NAME)
}

/// Unix seconds, formatted without pulling in a date library.
fn timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Locate the real cargo on PATH, skipping this executable.
///
/// We canonicalise every candidate and compare against our own canonical
/// path, so a symlinked or copied shim is still recognised as "us".
fn find_real_cargo() -> Option<PathBuf> {
    let me = std::env::current_exe()
        .ok()
        .and_then(|p| std::fs::canonicalize(&p).ok());

    let exe_name = if cfg!(windows) { "cargo.exe" } else { "cargo" };
    let path_var = std::env::var_os("PATH")?;

    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(exe_name);
        if !candidate.is_file() {
            continue;
        }
        // Skip ourselves.
        if let (Some(me), Ok(canon)) = (&me, std::fs::canonicalize(&candidate)) {
            if canon == *me {
                continue;
            }
        }
        return Some(candidate);
    }
    None
}