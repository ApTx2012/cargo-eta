# cargo-eta

> A Cargo subcommand that shows **real-time build progress** together with an
> **estimated time remaining (ETA)** — and gets more accurate the more you build.

Cargo tells you *which* crate it is compiling, but not how far along the whole
build is, nor how much longer you will wait. `cargo-eta` fills that gap: it
wraps `cargo build` / `check` / `test` / `clippy`, consumes Cargo's
machine-readable JSON message stream, and renders a progress bar with an ETA.

The interesting part is the ETA. Different crates take wildly different amounts
of time to compile, so naive "done / total × elapsed" extrapolation is badly
wrong. `cargo-eta` keeps a persistent per-crate timing model and uses it to
estimate the units that have not finished yet.

## How the ETA is computed

At any moment each planned compilation unit is in one of four states, and the
total estimated build time is the sum of their contributions:

| State      | Contribution to the estimate                          |
|------------|-------------------------------------------------------|
| Done       | the *actual* wall-clock time it took                   |
| Running    | `expected − already_spent` (floored at half the estimate if it overran) |
| Pending    | the crate's historical `expected` time                 |
| Fresh      | zero (Cargo reused a cached artifact)                  |

```
total_estimate = completed_actual
               + Σ running_remaining
               + Σ pending_expected
ETA            = total_estimate − elapsed
```

`expected` comes from the persistent model. When a crate has never been seen,
a conservative default is used so the first build's ETA errs on the side of
"too long".

## The timing model

The model lives at `~/.cargo/eta-cache.json` (override with `CARGO_HOME`).
Each compilation unit is keyed by:

- crate name and version
- build profile (`dev` / `release` / …)
- target triple
- whether it is a proc-macro

After a **successful** build, every unit's real duration is folded back into
the model with an exponential weighted moving average (α = 0.3), so a single
slow build — a machine hiccup, an antivirus scan — only nudges the estimate
rather than poisoning it.

A build that **fails** does not update the model at all: timings from a failed
build are not representative.

## Installation

```sh
cargo install --path .
```

This produces a `cargo-eta` binary. Because Cargo looks for `cargo-<name>`
executables on `PATH`, it can then be invoked either way:

```sh
cargo eta build        # via the cargo subcommand form
cargo-eta build        # directly
```

## Usage

```
cargo eta <SUBCOMMAND> [CARGO FLAGS...]
cargo-eta <SUBCOMMAND> [CARGO FLAGS...]

SUBCOMMANDS:
    build, check, test, clippy

OPTIONS:
    -q, --quiet                Only print the final summary
        --json                 Emit machine-readable progress events
        --eta-update-every <N> Refresh progress every N units (default 1)
    -h, --help                 Print help
    -V, --version              Print version
```

Any other flag is forwarded verbatim to the underlying cargo invocation, e.g.:

```sh
cargo eta build --release
cargo eta check --all-targets
cargo eta test --no-run
```

### Output modes

- **Interactive terminal** — a live progress bar on stderr showing percent,
  completed/total units, ETA, elapsed time, and the crate currently compiling.
- **Non-TTY (CI logs)** — the bar is replaced by a throttled plain line
  (roughly every 5 seconds) so logs stay readable:
  ```
  [cargo-eta]  42% (37/88) elapsed 12s eta 18s | syn
  ```
- **`--quiet`** — no per-unit output, only the final summary.
- **`--json`** — a stream of JSON progress events on stdout, for CI or tooling:
  ```json
  {"type":"progress","percent":42.05,"done":37,"total":88,"running":2,"fresh":0,"elapsed":12.004,"eta":18.21,"current":"syn"}
  {"type":"finished","success":true,"elapsed":30.77,"total":88,"done":88}
  ```

## Accurate percentages from the start

To know the percentage, the tool needs the total number of compilation units
up front. It tries `cargo <subcommand> --unit-graph -Z unstable-options`,
which is **nightly-only**. When that is unavailable (stable toolchain), it
falls back to discovering units as they appear — the build still works, but
early percentages are approximate. The fallback is announced on stderr.

## Why JSON events, not parsed text

`cargo-eta` never parses Cargo's human-readable output; that is far too
brittle. It always runs Cargo with `--message-format=json` and reacts to
`compiler-artifact`, `build-script-executed`, and `build-finished` events.

## Status

Early prototype. The build/check/test/clippy paths, the persistent model, and
the three output modes are implemented; the model's predictive accuracy has
not yet been benchmarked on a large workspace.

## License

MIT OR Apache-2.0.