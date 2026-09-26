use anyhow::{bail, Result};

use crate::render::OutputMode;

/// Parsed command line.
pub struct Args {
    pub subcommand: String,
    pub passthrough: Vec<String>,
    pub output: OutputMode,
    /// How many unit completions between forced progress refreshes.
    pub update_every: usize,
}

const SUPPORTED: &[&str] = &["build", "check", "test", "clippy"];

impl Args {
    /// Parse `std::env::args()`.
    ///
    /// Two invocation shapes are supported:
    ///   cargo eta build --release   -> ["cargo-eta", "eta", "build", "--release"]
    ///   cargo-eta build --release   -> ["cargo-eta", "build", "--release"]
    /// We detect and drop the extra "eta" token in the first form.
    pub fn parse() -> Result<Args> {
        let mut argv: Vec<String> = std::env::args().collect();
        // argv[0] is the program path; drop it.
        if !argv.is_empty() {
            argv.remove(0);
        }

        // `cargo eta ...` passes the subcommand alias as the first argument.
        if argv.first().map(String::as_str) == Some("eta") {
            argv.remove(0);
        }

        let mut output = OutputMode::Auto;
        let mut update_every = 1usize;
        let mut subcommand: Option<String> = None;
        let mut passthrough = Vec::new();

        let mut iter = argv.into_iter();
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--quiet" | "-q" => output = OutputMode::Quiet,
                "--json" => output = OutputMode::Json,
                "--eta-update-every" => {
                    if let Some(v) = iter.next() {
                        update_every = v.parse().unwrap_or(1).max(1);
                    }
                }
                "--help" | "-h" => {
                    print_help();
                    std::process::exit(0);
                }
                "--version" | "-V" => {
                    println!("cargo-eta {}", env!("CARGO_PKG_VERSION"));
                    std::process::exit(0);
                }
                other if other.starts_with('-') && subcommand.is_none() => {
                    bail!("unexpected flag before subcommand: {other}");
                }
                other if subcommand.is_none() => {
                    subcommand = Some(other.to_string());
                }
                other => passthrough.push(other.to_string()),
            }
        }

        let subcommand = match subcommand {
            Some(s) => s,
            None => {
                print_help();
                std::process::exit(2);
            }
        };

        if !SUPPORTED.contains(&subcommand.as_str()) {
            bail!(
                "unsupported subcommand `{subcommand}` (expected one of: {})",
                SUPPORTED.join(", ")
            );
        }

        Ok(Args {
            subcommand,
            passthrough,
            output,
            update_every,
        })
    }
}

fn print_help() {
    println!(
        "cargo-eta {ver}
Real-time Cargo build progress with an estimated time remaining.

USAGE:
    cargo eta <SUBCOMMAND> [CARGO FLAGS...]
    cargo-eta <SUBCOMMAND> [CARGO FLAGS...]

SUBCOMMANDS:
    build, check, test, clippy

OPTIONS:
    -q, --quiet                Only print the final summary
        --json                 Emit machine-readable progress events
        --eta-update-every <N> Refresh progress every N units (default 1)
    -h, --help                 Print this help
    -V, --version              Print version

All other flags are forwarded to the underlying cargo invocation.",
        ver = env!("CARGO_PKG_VERSION")
    );
}