//! Unexpected errors for pyfa's error dialog (gui/errorDialog.py): files that could not be read
//! or written, and panics. They queue up here, wherever they happen, and the main window shows
//! them. A panic that takes efa down is written to a crash file and shown on the next start.

use std::{
    fmt::Write as _,
    path::PathBuf,
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
};

/// One error, with everything worth pasting into a bug report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// What went wrong, in a line.
    pub title: String,
    pub details: String,
}

static PENDING: Mutex<Vec<Report>> = Mutex::new(Vec::new());
/// A panic of this run is in the crash file; once shown, efa lived through it.
static CRASH_WRITTEN: AtomicBool = AtomicBool::new(false);

/// Queue an error for the error dialog, and log it.
pub fn report(title: impl Into<String>, details: impl Into<String>) {
    let report = Report {
        title: title.into(),
        details: details.into(),
    };
    eprintln!("{}: {}", report.title, report.details);
    PENDING
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(report);
}

/// The errors queued since last asked, oldest first.
pub fn take() -> Vec<Report> {
    let taken = std::mem::take(&mut *PENDING.lock().unwrap_or_else(PoisonError::into_inner));
    // Shown now, so the next start need not show it again.
    if !taken.is_empty()
        && CRASH_WRITTEN.swap(false, Ordering::Relaxed)
        && let Some(path) = crash_file()
    {
        let _ = std::fs::remove_file(path);
    }
    taken
}

/// Queue panics as errors and keep them in the crash file, after the default hook has printed
/// them.
pub fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default(info);
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic".to_owned());
        let mut details = message;
        if let Some(location) = info.location() {
            let _ = write!(details, "\nat {location}");
        }
        let thread = std::thread::current();
        let _ = write!(
            details,
            "\nthread: {}\nefa {}\n\n{}",
            thread.name().unwrap_or("unnamed"),
            env!("CARGO_PKG_VERSION"),
            std::backtrace::Backtrace::force_capture()
        );
        if let Some(path) = crash_file() {
            let _ = std::fs::create_dir_all(path.parent().unwrap_or(&path));
            if std::fs::write(&path, &details).is_ok() {
                CRASH_WRITTEN.store(true, Ordering::Relaxed);
            }
        }
        PENDING
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Report {
                title: "efa ran into an unexpected error".to_owned(),
                details,
            });
    }));
}

/// The panic that ended the last run, if any, queued for the error dialog. The crash file is
/// removed so it shows once.
pub fn queue_last_crash() {
    let Some(path) = crash_file() else { return };
    let Ok(details) = std::fs::read_to_string(&path) else {
        return;
    };
    let _ = std::fs::remove_file(&path);
    PENDING
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(Report {
            title: "efa closed after an unexpected error last time".to_owned(),
            details,
        });
}

fn crash_file() -> Option<PathBuf> {
    crate::paths::data_file("crash.txt")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_queue_until_taken() {
        report("saving fits", "disk full");
        let taken = take();
        assert!(taken.iter().any(|r| r.details == "disk full"));
        assert!(!take().iter().any(|r| r.details == "disk full"));
    }
}
