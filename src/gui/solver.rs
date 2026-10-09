use iced::Task;
use tally_problem::{SearchProgress, TallyCounter};

#[cfg(not(target_arch = "wasm32"))]
#[path = "solver_native.rs"]
mod backend;
#[cfg(target_arch = "wasm32")]
#[path = "solver_wasm.rs"]
mod backend;

pub use backend::Control;

/// Start a cancellable background search and deliver its updates as an Iced task.
/// The search, including its initial allocation, never runs on the UI thread.
pub fn start(
    counter: TallyCounter,
    target: Vec<u8>,
) -> (Control, Task<Result<SearchProgress, String>>) {
    backend::start(counter, target)
}
