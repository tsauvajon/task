//! Docker-Compose-style progress reporter for batch CLI operations.
//!
//! One row per work item, pinned in place, updated concurrently from
//! rayon workers. Gracefully degrades to plain logging when stdout is
//! not a TTY or the terminal is too small to fit the full block.
//!
//! Used by `task detach update` and `task detach install`. Single-item
//! commands (`update_one`, `install_one`) intentionally do not use this —
//! their plain log output is already the right UX for one target.

use std::{
    io::IsTerminal,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};

use crate::runtime::spinner::FRAMES_STR;

/// Phase of a running work item. Kept as an enum rather than a free-form
/// string so the renderer owns the wording and we get a fixed vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Fetching,
    Resetting,
    Installing,
}

impl Phase {
    fn label(self) -> &'static str {
        match self {
            Self::Fetching => "Fetching",
            Self::Resetting => "Resetting",
            Self::Installing => "Installing",
        }
    }
}

/// Terminal state of a work item. Decides which glyph the final row
/// displays and whether we consider the item a failure.
#[derive(Debug, Clone)]
pub enum Outcome {
    Succeeded { note: &'static str },
    Failed { message: String },
    Skipped { reason: &'static str },
}

impl Outcome {
    pub fn is_failure(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }
}

/// Whether the reporter is actually drawing to the terminal (TTY mode)
/// or silently collecting state for a plain-log fallback (headless mode).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Tty,
    Headless,
}

/// Per-row shared state. `Option<Instant>` rather than bare `Instant`
/// because the row starts in a pending state before any worker claims it.
#[derive(Debug)]
struct RowState {
    label: String,
    label_width: usize,
    started_at: Option<Instant>,
    finished_at: Option<Instant>,
    current_phase: Option<Phase>,
    outcome: Option<Outcome>,
}

impl RowState {
    fn new(label: String, label_width: usize) -> Self {
        Self {
            label,
            label_width,
            started_at: None,
            finished_at: None,
            current_phase: None,
            outcome: None,
        }
    }

    /// Elapsed time between start and either now (still running) or
    /// finish (completed). Returns zero while pending.
    fn elapsed(&self) -> Duration {
        match (self.started_at, self.finished_at) {
            (Some(start), Some(end)) => end.duration_since(start),
            (Some(start), None) => start.elapsed(),
            _ => Duration::ZERO,
        }
    }
}

/// Batch progress reporter. Spawn once per batch, hand out `RowHandle`s
/// to workers, and call [`ProgressReporter::finish`] to render the final
/// snapshot and drop the animation.
pub struct ProgressReporter {
    mode: Mode,
    multi: Option<MultiProgress>,
    header: Option<ProgressBar>,
    rows: Vec<Row>,
    title: String,
    total: usize,
    completed: Arc<Mutex<usize>>,
}

struct Row {
    bar: Option<ProgressBar>,
    state: Arc<Mutex<RowState>>,
}

/// Worker-side handle for one row. Cheap to clone; all state mutations
/// go through the shared `Arc<Mutex<...>>` behind the handle.
pub struct RowHandle {
    state: Arc<Mutex<RowState>>,
    bar: Option<ProgressBar>,
    completed: Arc<Mutex<usize>>,
    header: Option<ProgressBar>,
    total: usize,
    title: String,
    finalized: bool,
}

impl ProgressReporter {
    /// Build a new reporter.
    ///
    /// * `title` — verb shown in the header, e.g. `"Updating"`.
    /// * `labels` — one string per work item, rendered in the row prefix.
    pub fn new(title: impl Into<String>, labels: Vec<String>) -> Self {
        let title: String = title.into();
        let total = labels.len();
        let completed = Arc::new(Mutex::new(0usize));
        let mode = detect_mode(total);

        let (multi, header, rows) = match mode {
            Mode::Tty => build_tty(&title, &labels, total),
            Mode::Headless => build_headless(&labels),
        };

        Self {
            mode,
            multi,
            header,
            rows,
            title,
            total,
            completed,
        }
    }

    /// Claim the row for `index`. The returned handle mutates the row's
    /// state; the renderer thread (owned by indicatif) picks up changes
    /// via the steady-tick timer.
    pub fn begin(&self, index: usize) -> RowHandle {
        let row = &self.rows[index];
        {
            let mut state = row.state.lock().expect("row state poisoned");
            state.started_at = Some(Instant::now());
            state.current_phase = Some(Phase::Fetching);
        }
        if let Some(bar) = &row.bar {
            bar.set_message(render_row_message(&row.state));
        }
        RowHandle {
            state: Arc::clone(&row.state),
            bar: row.bar.clone(),
            completed: Arc::clone(&self.completed),
            header: self.header.clone(),
            total: self.total,
            title: self.title.clone(),
            finalized: false,
        }
    }

    /// Finish the batch. In TTY mode, leaves the final snapshot drawn
    /// on-screen; in headless mode, prints one plain line per item.
    pub fn finish(self) {
        match self.mode {
            Mode::Tty => {
                if let Some(header) = &self.header {
                    header.set_message(format!(
                        "{}/{} complete",
                        *self.completed.lock().unwrap(),
                        self.total,
                    ));
                    header.finish();
                }
                for row in &self.rows {
                    if let Some(bar) = &row.bar {
                        bar.set_message(render_row_message(&row.state));
                        bar.finish();
                    }
                }
                drop(self.multi);
            }
            Mode::Headless => {
                // In headless mode rows carry no bar; print one summary
                // line per row so CI logs record the final state.
                for row in &self.rows {
                    let state = row.state.lock().unwrap();
                    println!("{}", render_headless_line(&state));
                }
            }
        }
    }

    #[cfg(test)]
    fn snapshot(&self) -> Vec<RowSnapshot> {
        self.rows
            .iter()
            .map(|row| {
                let s = row.state.lock().unwrap();
                RowSnapshot {
                    label: s.label.clone(),
                    phase: s.current_phase,
                    outcome: s.outcome.clone(),
                    elapsed: s.elapsed(),
                }
            })
            .collect()
    }
}

impl RowHandle {
    /// Advance this row to a new phase while it keeps running.
    pub fn phase(&self, phase: Phase) {
        if self.finalized {
            return;
        }
        {
            let mut state = self.state.lock().expect("row state poisoned");
            state.current_phase = Some(phase);
        }
        if let Some(bar) = &self.bar {
            bar.set_message(render_row_message(&self.state));
        }
    }

    /// Mark the row as a success. `note` is the short outcome text
    /// shown at the right of the row (e.g. `"Updated"`, `"Installed"`).
    pub fn succeeded(mut self, note: &'static str) {
        self.finalize(Outcome::Succeeded { note });
    }

    /// Mark the row as a failure with a user-facing message.
    pub fn failed(mut self, message: impl Into<String>) {
        self.finalize(Outcome::Failed {
            message: message.into(),
        });
    }

    /// Mark the row as skipped with a short reason.
    pub fn skipped(mut self, reason: &'static str) {
        self.finalize(Outcome::Skipped { reason });
    }

    fn finalize(&mut self, outcome: Outcome) {
        if self.finalized {
            return;
        }
        self.finalized = true;
        {
            let mut state = self.state.lock().expect("row state poisoned");
            state.finished_at = Some(Instant::now());
            state.current_phase = None;
            state.outcome = Some(outcome);
        }
        if let Some(bar) = &self.bar {
            // Swap to a spinner-less template so the finished row shows a
            // static outcome glyph instead of a still-animating spinner.
            bar.set_style(finished_style());
            bar.set_message(render_row_message(&self.state));
        }
        let mut count = self.completed.lock().unwrap();
        *count += 1;
        if let Some(header) = &self.header {
            header.set_message(format!("{} {}/{}", self.title, *count, self.total));
        }
    }
}

/// Style for a finished row — no spinner, just the rendered message
/// which carries its own outcome glyph (✓ / ✗ / ∘).
fn finished_style() -> ProgressStyle {
    ProgressStyle::with_template(" {msg}").unwrap_or_else(|_| ProgressStyle::default_spinner())
}

impl Drop for RowHandle {
    fn drop(&mut self) {
        // If a worker panicked or forgot to finalise, record the row as
        // a failure so the final snapshot is coherent.
        if !self.finalized {
            self.finalize(Outcome::Failed {
                message: "worker dropped handle without reporting outcome".into(),
            });
        }
    }
}

/// Decide whether we should render a TTY progress block.
///
/// Headless when:
/// - stdout isn't a TTY (piped, redirected to a file, CI),
/// - `NO_COLOR` env var is set,
/// - `TERM=dumb`,
/// - the terminal is too short to fit the header plus every row without
///   scrolling (scrolling would ruin in-place updates).
fn detect_mode(row_count: usize) -> Mode {
    if !std::io::stdout().is_terminal() {
        return Mode::Headless;
    }
    if std::env::var_os("NO_COLOR").is_some() {
        return Mode::Headless;
    }
    if std::env::var("TERM").ok().as_deref() == Some("dumb") {
        return Mode::Headless;
    }
    match crossterm::terminal::size() {
        Ok((_, rows)) if (rows as usize) < row_count.saturating_add(2) => Mode::Headless,
        _ => Mode::Tty,
    }
}

fn build_tty(
    title: &str,
    labels: &[String],
    total: usize,
) -> (Option<MultiProgress>, Option<ProgressBar>, Vec<Row>) {
    let multi = MultiProgress::with_draw_target(ProgressDrawTarget::stderr_with_hz(10));

    let header_style = ProgressStyle::with_template("[+] {msg}")
        .unwrap_or_else(|_| ProgressStyle::default_spinner());
    let header = multi.add(ProgressBar::new_spinner());
    header.set_style(header_style);
    header.set_message(format!("{title} 0/{total}"));

    let row_style = ProgressStyle::with_template(" {spinner:.cyan} {msg}")
        .unwrap_or_else(|_| ProgressStyle::default_spinner())
        .tick_strings(FRAMES_STR);

    let longest = labels.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let label_width = longest.max(20);
    let rows: Vec<Row> = labels
        .iter()
        .map(|label| {
            let state = Arc::new(Mutex::new(RowState::new(label.clone(), label_width)));
            let bar = multi.add(ProgressBar::new_spinner());
            bar.set_style(row_style.clone());
            bar.set_message(render_row_message(&state));
            bar.enable_steady_tick(Duration::from_millis(100));
            Row {
                bar: Some(bar),
                state,
            }
        })
        .collect();

    header.enable_steady_tick(Duration::from_millis(100));

    (Some(multi), Some(header), rows)
}

fn build_headless(labels: &[String]) -> (Option<MultiProgress>, Option<ProgressBar>, Vec<Row>) {
    let longest = labels.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let label_width = longest.max(20);
    let rows: Vec<Row> = labels
        .iter()
        .map(|label| Row {
            bar: None,
            state: Arc::new(Mutex::new(RowState::new(label.clone(), label_width))),
        })
        .collect();
    (None, None, rows)
}

/// Format the indicatif message portion (excluding the leading spinner).
/// The spinner glyph comes from the `{spinner}` template slot and
/// animates automatically while the bar is running.
fn render_row_message(state: &Mutex<RowState>) -> String {
    let state = state.lock().expect("row state poisoned");
    let label_width = state.label_width;
    let right = match (&state.outcome, state.current_phase) {
        (Some(Outcome::Succeeded { note }), _) => {
            format!("✓ {note:<10}  {:.1}s", state.elapsed().as_secs_f64())
        }
        (Some(Outcome::Failed { message }), _) => format!(
            "✗ Failed       {:.1}s  ({})",
            state.elapsed().as_secs_f64(),
            short(message, 48),
        ),
        (Some(Outcome::Skipped { reason }), _) => format!("∘ Skipped      ({reason})"),
        (None, Some(phase)) => format!(
            "  {:<13}  {:.1}s",
            phase.label(),
            state.elapsed().as_secs_f64()
        ),
        (None, None) => "  Pending".to_string(),
    };
    format!("{:<width$}  {}", state.label, right, width = label_width)
}

fn render_headless_line(state: &RowState) -> String {
    let tag = match &state.outcome {
        Some(Outcome::Succeeded { note }) => format!("✓ {note}"),
        Some(Outcome::Failed { message }) => format!("✗ Failed: {message}"),
        Some(Outcome::Skipped { reason }) => format!("∘ Skipped ({reason})"),
        None => "… Pending".to_string(),
    };
    format!(
        "{}  {} ({:.1}s)",
        state.label,
        tag,
        state.elapsed().as_secs_f64()
    )
}

fn short(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
#[derive(Debug, Clone)]
struct RowSnapshot {
    #[allow(dead_code)] // read via Debug output during test failures.
    label: String,
    phase: Option<Phase>,
    outcome: Option<Outcome>,
    elapsed: Duration,
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Duration};

    use super::{Outcome, Phase, ProgressReporter, short};

    fn make(labels: &[&str]) -> ProgressReporter {
        ProgressReporter::new(
            "Updating",
            labels.iter().map(|s| (*s).to_string()).collect(),
        )
    }

    mod transitions {
        use super::*;

        #[test]
        fn begin_marks_row_running_with_fetching_phase() {
            let progress = make(&["repo-a"]);
            let _handle = progress.begin(0);
            let snap = progress.snapshot();
            assert_eq!(snap[0].phase, Some(Phase::Fetching));
            assert!(snap[0].outcome.is_none());
        }

        #[test]
        fn phase_transitions_update_state() {
            let progress = make(&["repo-a"]);
            let handle = progress.begin(0);
            handle.phase(Phase::Installing);
            let snap = progress.snapshot();
            assert_eq!(snap[0].phase, Some(Phase::Installing));
        }

        #[test]
        fn succeeded_records_outcome_and_finishes_phase() {
            let progress = make(&["repo-a"]);
            let handle = progress.begin(0);
            handle.succeeded("Updated");
            let snap = progress.snapshot();
            assert!(matches!(
                snap[0].outcome,
                Some(Outcome::Succeeded { note: "Updated" })
            ));
            assert!(snap[0].phase.is_none());
        }

        #[test]
        fn failed_records_error_message() {
            let progress = make(&["repo-a"]);
            let handle = progress.begin(0);
            handle.failed("fetch 404");
            let snap = progress.snapshot();
            match &snap[0].outcome {
                Some(Outcome::Failed { message }) => assert_eq!(message, "fetch 404"),
                other => panic!("expected Failed, got {other:?}"),
            }
        }

        #[test]
        fn skipped_records_reason() {
            let progress = make(&["repo-a"]);
            let handle = progress.begin(0);
            handle.skipped("no install entry");
            let snap = progress.snapshot();
            assert!(matches!(
                snap[0].outcome,
                Some(Outcome::Skipped {
                    reason: "no install entry"
                })
            ));
        }

        #[test]
        fn drop_without_finalize_marks_failure() {
            let progress = make(&["repo-a"]);
            {
                let _handle = progress.begin(0);
                // handle dropped without .succeeded/.failed/.skipped
            }
            let snap = progress.snapshot();
            assert!(matches!(snap[0].outcome, Some(Outcome::Failed { .. })));
        }
    }

    mod elapsed {
        use super::*;

        #[test]
        fn elapsed_is_zero_before_begin() {
            let progress = make(&["repo-a"]);
            assert_eq!(progress.snapshot()[0].elapsed, Duration::ZERO);
        }

        #[test]
        fn elapsed_grows_while_running() {
            let progress = make(&["repo-a"]);
            let _handle = progress.begin(0);
            thread::sleep(Duration::from_millis(20));
            let elapsed = progress.snapshot()[0].elapsed;
            assert!(
                elapsed >= Duration::from_millis(15),
                "expected elapsed >= 15ms, got {elapsed:?}"
            );
        }

        #[test]
        fn elapsed_freezes_after_finalize() {
            let progress = make(&["repo-a"]);
            let handle = progress.begin(0);
            thread::sleep(Duration::from_millis(10));
            handle.succeeded("Updated");
            let first = progress.snapshot()[0].elapsed;
            thread::sleep(Duration::from_millis(20));
            let second = progress.snapshot()[0].elapsed;
            assert_eq!(
                first, second,
                "elapsed should stop advancing after finalize"
            );
        }
    }

    mod outcome_helpers {
        use super::*;

        #[test]
        fn failure_predicate_covers_failed_only() {
            assert!(
                Outcome::Failed {
                    message: "x".into(),
                }
                .is_failure()
            );
            assert!(!Outcome::Succeeded { note: "Updated" }.is_failure());
            assert!(!Outcome::Skipped { reason: "no entry" }.is_failure());
        }
    }

    mod short_helper {
        use super::short;

        #[test]
        fn short_string_is_unchanged() {
            assert_eq!(short("abc", 10), "abc");
        }

        #[test]
        fn long_string_is_truncated_with_ellipsis() {
            let out = short("abcdefghij", 5);
            assert_eq!(out.chars().count(), 5);
            assert!(out.ends_with('…'));
        }

        #[test]
        fn exact_length_is_preserved() {
            assert_eq!(short("abcde", 5), "abcde");
        }
    }

    mod parallel {
        use std::{
            sync::{Arc, atomic::AtomicUsize},
            thread,
        };

        use super::*;

        #[test]
        fn concurrent_workers_can_finalize_in_any_order() {
            let progress = Arc::new(make(&["a", "b", "c", "d"]));
            let counter = Arc::new(AtomicUsize::new(0));

            let mut handles = Vec::new();
            for i in 0..4 {
                let progress = Arc::clone(&progress);
                let counter = Arc::clone(&counter);
                handles.push(thread::spawn(move || {
                    let h = progress.begin(i);
                    thread::sleep(Duration::from_millis(5 * (i as u64 + 1)));
                    h.succeeded("Updated");
                    counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }));
            }
            for h in handles {
                h.join().unwrap();
            }
            assert_eq!(counter.load(std::sync::atomic::Ordering::Relaxed), 4);
            let snap = progress.snapshot();
            for row in snap {
                assert!(matches!(row.outcome, Some(Outcome::Succeeded { .. })));
            }
        }
    }
}
