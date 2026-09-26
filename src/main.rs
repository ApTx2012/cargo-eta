mod cargo_invoke;
mod cli;
mod error;
mod eta;
mod event;
mod model;
mod render;
mod unit_graph;

use std::process::ExitCode;

use anyhow::Result;

use crate::cargo_invoke::Invocation;
use crate::cli::Args;
use crate::eta::EtaCalculator;
use crate::model::TimingModel;
use crate::render::Renderer;

fn main() -> ExitCode {
    match run() {
        Ok(success) => {
            if success {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(e) => {
            eprintln!("cargo-eta: error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<bool> {
    let args = Args::parse()?;
    let cache = model::cache_path();
    let mut timing = TimingModel::load(&cache);

    let inv = Invocation {
        subcommand: &args.subcommand,
        passthrough: &args.passthrough,
    };

    // Prefer an accurate plan from --unit-graph (nightly). If unavailable,
    // start with an empty plan and discover units as events arrive.
    let planned_keys = match cargo_invoke::try_unit_graph(&inv) {
        Some(graph) => graph.planned_keys(&inv.profile_name(), &inv.target_triple()),
        None => Vec::new(),
    };
    let planned_from_graph = !planned_keys.is_empty();

    let mut calc = EtaCalculator::new(planned_keys, &timing);
    let mut renderer = Renderer::new(args.output);

    if !planned_from_graph {
        // Make the fallback visible but not alarming.
        eprintln!(
            "cargo-eta: --unit-graph unavailable (needs nightly); \
             falling back to dynamic discovery, early percentages are approximate"
        );
    }

    let outcome = cargo_invoke::run(
        &inv,
        &mut calc,
        &mut renderer,
        &timing,
        args.update_every,
    )?;

    if outcome.success {
        for (key, secs) in &outcome.observations {
            timing.record(&key.as_key(), *secs);
        }
        timing.save(&cache);
    } else {
        eprintln!("cargo-eta: build failed; timing model left unchanged");
    }

    Ok(outcome.success)
}