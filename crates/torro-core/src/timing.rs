//! Timestamps for measuring how fast the windows start and load (issue #206, `docs/tempo.md`).
//!
//! Silent unless `TORRO_TIMING` is set to something non-empty; it is read once, the first time
//! anything here is called. Set, every mark is one line on stderr,
//! `torro-timing <program> +<ms>ms <step>`, the time counted from [`start`], which each program
//! calls first thing in `main`. `scripts/measure-start.sh` reads these lines.
//!
//! The marks name steps, counts and durations only, never what a window shows (subjects, senders,
//! folder names): stderr may end up in a log someone shares.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::io::Write;
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

/// The variable that turns the marks on.
pub const VARIABLE: &str = "TORRO_TIMING";

/// From how long on [`slow`] reports a step: one frame at 60 Hz.
pub const SLOW: Duration = Duration::from_millis(16);

static CLOCK: OnceLock<Option<Clock>> = OnceLock::new();

/// Starts the clock the marks count from. Call it first in `main`; without it the clock starts at
/// the first mark.
pub fn start() {
    if let Some(clock) = clock() {
        clock.mark("start");
    }
}

/// Writes `step` with the time since [`start`].
pub fn mark(step: &str) {
    if let Some(clock) = clock() {
        clock.mark(step);
    }
}

/// Writes `step` the first time it is called with it in this process, later calls nothing: the
/// first frame, the first data.
pub fn once(step: &'static str) {
    if let Some(clock) = clock() {
        clock.once(step);
    }
}

/// Writes `step` with how long it took since `since`, only when that is at least [`SLOW`]: what
/// keeps a window from drawing its next frame.
pub fn slow(step: &str, since: Instant) {
    if let Some(clock) = clock() {
        clock.slow(step, since.elapsed());
    }
}

/// Writes `step` with how long it took since `since`, however short: a load of the data.
pub fn took(step: &str, since: Instant) {
    if let Some(clock) = clock() {
        clock.took(step, since.elapsed());
    }
}

/// Whether marks are written: [`VARIABLE`] set to something non-empty.
pub fn enabled() -> bool {
    clock().is_some()
}

fn clock() -> Option<&'static Clock> {
    CLOCK
        .get_or_init(|| {
            let on = is_on(std::env::var_os(VARIABLE).as_deref());
            on.then(|| Clock::new(program(), Box::new(std::io::stderr())))
        })
        .as_ref()
}

/// Whether the value of [`VARIABLE`] turns the marks on.
fn is_on(value: Option<&OsStr>) -> bool {
    value.is_some_and(|value| !value.is_empty())
}

/// The name of the running program, as the lines name it.
fn program() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.file_stem().map(|stem| stem.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "torro".to_string())
}

/// The clock of one process and where its lines go.
struct Clock {
    started: Instant,
    program: String,
    out: Mutex<Box<dyn Write + Send>>,
    seen: Mutex<HashSet<&'static str>>,
}

impl Clock {
    fn new(program: String, out: Box<dyn Write + Send>) -> Clock {
        Clock { started: Instant::now(), program, out: Mutex::new(out), seen: Mutex::new(HashSet::new()) }
    }

    fn mark(&self, step: &str) {
        self.write(step, None);
    }

    fn once(&self, step: &'static str) {
        let first = self.seen.lock().unwrap_or_else(PoisonError::into_inner).insert(step);
        if first {
            self.write(step, None);
        }
    }

    fn slow(&self, step: &str, took: Duration) {
        if took >= SLOW {
            self.write(step, Some(took));
        }
    }

    fn took(&self, step: &str, took: Duration) {
        self.write(step, Some(took));
    }

    fn write(&self, step: &str, took: Option<Duration>) {
        let line = line(&self.program, self.started.elapsed(), step, took);
        let mut out = self.out.lock().unwrap_or_else(PoisonError::into_inner);
        // A closed stderr loses the mark, never the program.
        let _ = writeln!(out, "{line}");
    }
}

/// One line: `torro-timing <program> +<ms>ms <step>`, with ` took=<ms>ms` when a duration is given.
fn line(program: &str, at: Duration, step: &str, took: Option<Duration>) -> String {
    let at = at.as_secs_f64() * 1000.0;
    match took {
        Some(took) => format!("torro-timing {program} +{at:.1}ms {step} took={:.1}ms", took.as_secs_f64() * 1000.0),
        None => format!("torro-timing {program} +{at:.1}ms {step}"),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    /// A writer the test can read back.
    #[derive(Clone, Default)]
    struct Lines(Arc<Mutex<Vec<u8>>>);

    impl Write for Lines {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("lock").extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Lines {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().expect("lock").clone()).expect("utf-8")
        }
    }

    fn clock() -> (Clock, Lines) {
        let lines = Lines::default();
        (Clock::new("torroinbox".into(), Box::new(lines.clone())), lines)
    }

    #[test]
    fn only_a_non_empty_variable_turns_the_marks_on() {
        assert!(!is_on(None));
        assert!(!is_on(Some(OsStr::new(""))));
        assert!(is_on(Some(OsStr::new("1"))));
    }

    #[test]
    fn a_mark_names_the_program_the_time_and_the_step() {
        assert_eq!(
            line("torroinbox", Duration::from_micros(12_345), "first frame", None),
            "torro-timing torroinbox +12.3ms first frame"
        );
        assert_eq!(
            line("torroinbox", Duration::from_millis(40), "load", Some(Duration::from_micros(3_300))),
            "torro-timing torroinbox +40.0ms load took=3.3ms"
        );
    }

    #[test]
    fn once_writes_a_step_only_the_first_time() {
        let (clock, lines) = clock();
        clock.once("first data");
        clock.once("first data");
        clock.once("first frame");
        let text = lines.text();
        assert_eq!(text.matches("first data").count(), 1, "{text}");
        assert_eq!(text.matches("first frame").count(), 1, "{text}");
    }

    #[test]
    fn slow_writes_only_what_takes_a_frame_or_longer() {
        let (clock, lines) = clock();
        clock.slow("view", Duration::from_millis(3));
        assert_eq!(lines.text(), "");
        clock.slow("view", SLOW);
        assert!(lines.text().contains("view took=16.0ms"), "{}", lines.text());
    }

    #[test]
    fn took_writes_every_duration() {
        let (clock, lines) = clock();
        clock.took("load", Duration::from_micros(400));
        assert!(lines.text().contains("load took=0.4ms"), "{}", lines.text());
    }
}
