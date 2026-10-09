//! Background search shared by the desktop and browser interfaces.
//!
//! Native builds use a dedicated thread; WASM builds use a Web Worker. Both
//! deliver paired progress, statistics, and elapsed time through an Iced task.
//! The caller retains the returned [`Control`] for the lifetime of its job and
//! drops it to cancel the search.

use std::time::Duration;

use iced::Task;
use tally_problem::{SearchProgress, SearchStatistics, TallyCounter};

#[cfg(not(target_arch = "wasm32"))]
#[path = "solver_native.rs"]
mod backend;
#[cfg(target_arch = "wasm32")]
#[path = "solver_wasm.rs"]
mod backend;

/// Keeps a background search alive and cancels it when dropped.
pub use backend::Control;

/// A search snapshot paired with the time measured by its background worker.
#[derive(Debug, Clone)]
pub struct Update {
    /// Current search status or its final result.
    pub progress: SearchProgress,
    /// Solver counters captured from the same batch as `progress`.
    pub statistics: SearchStatistics,
    /// Time measured inside the search backend, including solver initialization.
    ///
    /// Worker startup, WASM loading, and delivery to the UI are excluded.
    pub elapsed: Duration,
}

/// Start a cancellable background search and deliver its updates as an Iced task.
///
/// Search initialization and execution happen outside the UI thread. Retain the
/// control handle while consuming the task; dropping it cancels the job. The
/// stream ends after a final result or an error, or when cancelled.
pub fn start(counter: TallyCounter, target: Vec<u8>) -> (Control, Task<Result<Update, String>>) {
    backend::start(counter, target)
}
