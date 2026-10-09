//! Browser-only solver worker entry point, built separately from the GUI.
//!
//! Trunk emits the worker's WASM and bindings for `solver-worker.js` to load.
//! Native builds have an empty entry point and do not install a worker handler.

#[cfg(target_arch = "wasm32")]
#[path = "../gui/worker.rs"]
mod worker;

/// Install the request handler when loaded inside a browser worker.
fn main() {
    #[cfg(target_arch = "wasm32")]
    worker::install();
}
