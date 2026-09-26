use std::time::{Duration, Instant};

use crate::model::{TimingModel, UnitKey};

/// Lifecycle of a single compilation unit from the calculator's view.
#[derive(Debug, Clone)]
enum UnitState {
    /// Not seen yet in the event stream.
    Pending,
    /// Started; `started_at` is when we first saw activity for it.
    Running { started_at: Instant, expected: f64 },
    /// Finished; `actual` is the observed wall-clock duration.
    Done { actual: f64 },
    /// Finished, but cached/fresh — took effectively no time.
    Fresh,
}

/// Tracks in-flight and finished units and produces ETA snapshots.
///
/// The calculator is fed a plan (the keys Cargo intends to build) and then
/// a stream of events. It never mutates the timing model; updating the model
/// happens after the build, in `main`.
pub struct EtaCalculator {
    /// Planned units, in discovery order. Duplicates are allowed: a crate
    /// compiled as both lib and test appears twice.
    planned: Vec<PlannedUnit>,
    /// Observed wall-clock duration per finished unit (for the model update).
    observations: Vec<(UnitKey, f64)>,
    /// When the whole build started, for "elapsed" and ETA math.
    build_start: Instant,
}

#[derive(Debug, Clone)]
struct PlannedUnit {
    key: UnitKey,
    /// Expected seconds from the model; updated when a unit starts.
    expected: f64,
    state: UnitState,
}

/// A point-in-time view used by the renderer.
#[derive(Debug, Clone)]
pub struct ProgressSnapshot {
    pub total: usize,
    pub done: usize,
    pub running: usize,
    pub fresh: usize,
    pub percent: f64,
    pub elapsed: Duration,
    pub eta: Option<Duration>,
    pub current_crate: Option<String>,
}

impl EtaCalculator {
    /// Build a calculator from a pre-computed plan.
    ///
    /// `planned_keys` comes either from `--unit-graph` (accurate from t=0)
    /// or is empty, in which case units are discovered as events arrive.
    pub fn new(planned_keys: Vec<UnitKey>, model: &TimingModel) -> Self {
        let planned = planned_keys
            .into_iter()
            .map(|key| {
                let expected = model.expected_seconds(&key.as_key());
                PlannedUnit {
                    key,
                    expected,
                    state: UnitState::Pending,
                }
            })
            .collect();

        Self {
            planned,
            observations: Vec::new(),
            build_start: Instant::now(),
        }
    }

    /// Mark a unit as started. If it was not planned, it is appended.
    ///
    /// Cargo's JSON stream has no explicit "compilation started" event; the
    /// convention is to treat a unit's first appearance as its start, then
    /// immediately finish it when the artifact arrives.
    pub fn start_unit(&mut self, key: &UnitKey, model: &TimingModel) {
        if let Some(slot) = self
            .planned
            .iter_mut()
            .find(|p| p.key == *key && matches!(p.state, UnitState::Pending))
        {
            let expected = model.expected_seconds(&key.as_key());
            slot.expected = expected;
            slot.state = UnitState::Running {
                started_at: Instant::now(),
                expected,
            };
            return;
        }
        // Unknown unit: add it as running directly.
        let expected = model.expected_seconds(&key.as_key());
        self.planned.push(PlannedUnit {
            key: key.clone(),
            expected,
            state: UnitState::Running {
                started_at: Instant::now(),
                expected,
            },
        });
    }

    /// Mark a unit as finished, recording the real duration.
    ///
    /// `fresh` units were already compiled and Cargo did no work; we count
    /// them as done but do NOT feed them into the timing model (their near
    /// zero duration would poison the average).
    pub fn finish_unit(&mut self, key: &UnitKey, fresh: bool) {
        let idx = self
            .planned
            .iter()
            .position(|p| p.key == *key && matches!(p.state, UnitState::Running { .. }));

        match idx {
            Some(i) => {
                let actual = match &self.planned[i].state {
                    UnitState::Running { started_at, .. } => started_at.elapsed().as_secs_f64(),
                    _ => 0.0,
                };
                if fresh {
                    self.planned[i].state = UnitState::Fresh;
                } else {
                    self.planned[i].state = UnitState::Done { actual };
                    self.observations.push((key.clone(), actual));
                }
            }
            None => {
                if fresh {
                    return;
                }
                // Finished without a matching start (e.g. cached unit seen
                // only via its artifact). Record as done with zero duration.
                self.planned.push(PlannedUnit {
                    key: key.clone(),
                    expected: 0.0,
                    state: UnitState::Done { actual: 0.0 },
                });
            }
        }
    }

    /// Compute the current progress and ETA.
    pub fn snapshot(&self) -> ProgressSnapshot {
        let now = Instant::now();
        let elapsed = now.duration_since(self.build_start);

        let total = self.planned.len();
        let mut done = 0usize;
        let mut running = 0usize;
        let mut fresh = 0usize;
        let mut current_crate = None;

        // Per spec:
        //   total_est = completed_actual
        //             + sum over running of remaining
        //             + sum over pending of expected
        let mut completed_actual = 0.0f64;
        let mut running_remaining = 0.0f64;
        let mut pending_expected = 0.0f64;

        for unit in &self.planned {
            match &unit.state {
                UnitState::Pending => {
                    pending_expected += unit.expected;
                }
                UnitState::Running { started_at, expected } => {
                    running += 1;
                    if current_crate.is_none() {
                        current_crate = Some(unit.key.crate_name.clone());
                    }
                    let spent = started_at.elapsed().as_secs_f64();
                    // If the unit has already overrun its estimate, assume it
                    // still needs half the estimate rather than zero, so the
                    // ETA does not collapse to "instant".
                    let remaining = if spent >= *expected {
                        *expected * 0.5
                    } else {
                        *expected - spent
                    };
                    running_remaining += remaining;
                }
                UnitState::Done { actual } => {
                    done += 1;
                    completed_actual += actual;
                }
                UnitState::Fresh => {
                    done += 1;
                    fresh += 1;
                }
            }
        }

        let total_est = completed_actual + running_remaining + pending_expected;
        let eta_seconds = (total_est - elapsed.as_secs_f64()).max(0.0);
        let eta = if total > 0 && done >= total {
            Some(Duration::ZERO)
        } else {
            Some(Duration::from_secs_f64(eta_seconds))
        };

        let percent = if total == 0 {
            0.0
        } else {
            (done as f64 / total as f64) * 100.0
        };

        ProgressSnapshot {
            total,
            done,
            running,
            fresh,
            percent,
            elapsed,
            eta,
            current_crate,
        }
    }

    /// Take the observed durations to fold back into the persistent model,
    /// leaving the calculator empty. Only non-fresh, successfully-finished
    /// units appear here.
    pub fn drain_observations(&mut self) -> Vec<(UnitKey, f64)> {
        std::mem::take(&mut self.observations)
    }

    /// How many units are still unfinished (pending or running).
    pub fn remaining(&self) -> usize {
        self.planned
            .iter()
            .filter(|p| matches!(p.state, UnitState::Pending | UnitState::Running { .. }))
            .count()
    }
}