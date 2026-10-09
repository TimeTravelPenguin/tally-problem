#[cfg(target_arch = "wasm32")]
#[path = "../gui/worker.rs"]
mod worker;

fn main() {
    #[cfg(target_arch = "wasm32")]
    worker::install();
}
