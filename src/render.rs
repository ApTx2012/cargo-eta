use std::io::IsTerminal;
use std::time::{Duration, Instant};

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

use crate::eta::ProgressSnapshot;

/// How the tool reports progress to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    /// Dynamic bar on a TTY; periodic lines otherwise.
    Auto,
    /// No per-unit output, only the final summary.
    Quiet,
    /// Machine-readable JSON events on stdout.
    Json,
}

/// Formats a `Duration` as `1h02m03s` / `2m03s` / `03s`.
pub fn fmt_duration(d: Duration) -> String {
    let secs = d.as_secs();
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}h{m:02}m{s:02}s")
    } else if m > 0 {
        format!("{m}m{s:02}s")
    } else {
        format!("{s}s")
    }
}

/// Renders progress updates. Wraps the three output modes.
pub struct Renderer {
    mode: OutputMode,
    bar: Option<ProgressBar>,
    /// In non-TTY auto mode we throttle line output to this interval.
    last_line: Option<Instant>,
    line_interval: Duration,
    /// When true, `update` writes a JSON line to stdout.
    json: bool,
}

impl Renderer {
    /// Pick the right rendering strategy for the current environment.
    ///
    /// `Auto` means: dynamic bar if stdout is a real terminal, otherwise
    /// periodic plain lines suitable for CI logs.
    pub fn new(mode: OutputMode) -> Self {
        let is_tty = std::io::stdout().is_terminal();

        let bar = match mode {
            OutputMode::Quiet | OutputMode::Json => None,
            OutputMode::Auto if is_tty => Some(Self::make_bar()),
            OutputMode::Auto => None,
        };

        Self {
            mode,
            bar,
            last_line: None,
            line_interval: Duration::from_secs(5),
            json: mode == OutputMode::Json,
        }
    }

    fn make_bar() -> ProgressBar {
        let bar = ProgressBar::with_draw_target(None, ProgressDrawTarget::stderr());
        bar.set_style(
            ProgressStyle::with_template(
                "{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} ({percent}%) | ETA {eta} | {elapsed} | {msg}",
            )
            .expect("valid template")
            .progress_chars("=>-"),
        );
        bar.enable_steady_tick(Duration::from_millis(100));
        bar
    }

    /// Render one snapshot. Cheap enough to call on every event.
    pub fn update(&mut self, snap: &ProgressSnapshot) {
        if self.json {
            self.emit_json(snap);
            return;
        }
        if self.mode == OutputMode::Quiet {
            return;
        }

        match &self.bar {
            Some(bar) => {
                bar.set_length(snap.total as u64);
                bar.set_position(snap.done as u64);
                let msg = snap
                    .current_crate
                    .clone()
                    .unwrap_or_else(|| "waiting".to_string());
                let eta = snap
                    .eta
                    .map(fmt_duration)
                    .unwrap_or_else(|| "--".to_string());
                bar.set_message(format!(
                    "{} | ETA {} | {}",
                    msg,
                    eta,
                    fmt_duration(snap.elapsed)
                ));
            }
            None => self.maybe_print_line(snap),
        }
    }

    /// Non-TTY path: print a plain line at most once per `line_interval`.
    fn maybe_print_line(&mut self, snap: &ProgressSnapshot) {
        let now = Instant::now();
        let due = self
            .last_line
            .map(|t| now.duration_since(t) >= self.line_interval)
            .unwrap_or(true);
        if !due {
            return;
        }
        self.last_line = Some(now);

        let eta = snap
            .eta
            .map(fmt_duration)
            .unwrap_or_else(|| "--".to_string());
        let cur = snap.current_crate.as_deref().unwrap_or("waiting");
        println!(
            "[cargo-eta] {:>3.0}% ({}/{}) elapsed {} eta {} | {}",
            snap.percent,
            snap.done,
            snap.total,
            fmt_duration(snap.elapsed),
            eta,
            cur
        );
    }

    fn emit_json(&mut self, snap: &ProgressSnapshot) {
        // Hand-rolled JSON to avoid pulling serde into the hot path; the
        // shape is tiny and stable.
        let eta = snap
            .eta
            .map(|d| format!("{:.3}", d.as_secs_f64()))
            .unwrap_or_else(|| "null".to_string());
        let cur = match &snap.current_crate {
            Some(c) => format!("\"{}\"", c.replace('"', "\\\"")),
            None => "null".to_string(),
        };
        println!(
            "{{\"type\":\"progress\",\"percent\":{:.2},\"done\":{},\"total\":{},\"running\":{},\"fresh\":{},\"elapsed\":{:.3},\"eta\":{},\"current\":{}}}",
            snap.percent,
            snap.done,
            snap.total,
            snap.running,
            snap.fresh,
            snap.elapsed.as_secs_f64(),
            eta,
            cur
        );
    }

    /// Final summary line (printed in all modes except pure JSON).
    pub fn finish(&self, snap: &ProgressSnapshot, success: bool) {
        if let Some(bar) = &self.bar {
            bar.finish_and_clear();
        }
        if self.mode == OutputMode::Quiet || self.json {
            return;
        }
        let status = if success { "finished" } else { "FAILED" };
        let eta = snap
            .eta
            .map(fmt_duration)
            .unwrap_or_else(|| "--".to_string());
        println!(
            "[cargo-eta] build {} in {} ({} units, {} cached, final eta {}))",
            status,
            fmt_duration(snap.elapsed),
            snap.total,
            snap.fresh,
            eta
        );
    }

    /// Emit a terminal JSON event (only in JSON mode).
    pub fn finish_json(&self, snap: &ProgressSnapshot, success: bool) {
        if !self.json {
            return;
        }
        println!(
            "{{\"type\":\"finished\",\"success\":{},\"elapsed\":{:.3},\"total\":{},\"done\":{}}}",
            success,
            snap.elapsed.as_secs_f64(),
            snap.total,
            snap.done
        );
    }
}