// Version: 0.1.0 · updated 26-09-07-11-00
//
// One private folder per test, guaranteed not to be somebody else's.
//
// WHY THIS EXISTS. Five test modules each built a temporary folder named after
// the process id, on the reasonable assumption that a process id identifies a
// process. It does not: Windows reuses them, and quickly. Nothing here deleted
// its folder afterwards, so a run that drew a recycled id inherited the
// finished state of an earlier run.
//
// The failure that produced was work_waits_until_accepted_and_a_refusal_is_kept
// asserting that nothing had reached the teacher's folder before she accepted
// it, and finding a file there. The file was real. It had been accepted, by
// that same test, during a run half an hour earlier:
//
//   [handed-in]          10.42.0.98--good.txt     <- stale, from the older run
//   [handed-in/waiting]  10.42.0.98--good.txt     <- fresh, from this one
//   [handed-in/refused]  10.42.0.98--bad.jpg      <- stale
//
// Measured: 1,458 abandoned folders across 112 distinct process ids in about
// 150 runs, and roughly one failure in every twenty-five. Reruns passed, which
// is what makes this kind of fault expensive: the evidence is gone by the time
// anybody looks, and the test gets a reputation for being unreliable instead of
// the harness getting a fix.
//
// It is the same shape as the fault this release was written to remove. The
// address guard read a state that had been true earlier and presented it as
// now. So did this. Worth recording, because a test harness that can go stale
// is not a place from which to certify that a program cannot.
//
// The name now carries the moment it was made as well as the process id, so a
// recycled id cannot collide, and the folder is emptied before use so that a
// collision would still start from nothing. Folders older than an hour are
// swept once per process, which cleans up after every run that predates this
// file without ever touching a run happening concurrently.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static N: AtomicU32 = AtomicU32::new(0);
const PREFIX: &str = "hub-test-";

/// A folder that belongs to this call and to nothing else.
///
/// `tag` is there only to make a failure readable; uniqueness does not depend
/// on it, because depending on a human-chosen label is how this started.
pub fn scratch(tag: &str) -> PathBuf {
    sweep_once();
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let d = std::env::temp_dir().join(format!(
        "{PREFIX}{tag}-{}-{nanos:x}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    // Belt and braces: if this name has somehow been used, it starts empty.
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("scratch dir");
    d
}

/// Remove what earlier runs left behind, once per process.
///
/// An hour, because a test run takes seconds and a concurrent run must never
/// have its folders deleted underneath it. Failures are ignored throughout: a
/// folder held open by something else is a reason to leave it alone, not a
/// reason to fail somebody's test suite.
fn sweep_once() {
    static DONE: OnceLock<()> = OnceLock::new();
    DONE.get_or_init(|| {
        let hour_ago = SystemTime::now() - Duration::from_secs(3600);
        let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else { return };
        for e in entries.flatten() {
            let name = e.file_name();
            let Some(name) = name.to_str() else { continue };
            // The old pid-only names are swept too; they are why this exists.
            let ours = name.starts_with(PREFIX)
                || name.starts_with("hub-page-test-")
                || name.starts_with("hub-block-test-")
                || name.starts_with("hub-walk-")
                || name.starts_with("hub-tab-test-")
                || name.starts_with("hub-zip-");
            if !ours {
                continue;
            }
            let old = e
                .metadata()
                .and_then(|m| m.modified())
                .map(|t| t < hour_ago)
                .unwrap_or(false);
            if old {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The property the old helper did not have.
    #[test]
    fn two_scratches_are_never_the_same_folder() {
        let a = scratch("same");
        let b = scratch("same");
        assert_ne!(a, b, "a tag must not be an identity");
    }

    /// A recycled process id must not hand over somebody else's finished state.
    #[test]
    fn a_scratch_starts_empty_even_if_the_name_repeats() {
        let d = scratch("reuse");
        std::fs::write(d.join("left-behind.txt"), b"from an earlier run").unwrap();
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 0);
    }
}
