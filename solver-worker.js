// Trunk emits this worker's bindings under stable names beside this bootstrap.
// Wait for Rust's ready message before posting a search request.
try {
  importScripts("tally-worker.js");
  wasm_bindgen("tally-worker_bg.wasm").catch((error) => {
    self.postMessage(["error", `Unable to load the background solver: ${error}`]);
  });
} catch (error) {
  self.postMessage(["error", `Unable to load the background solver: ${error}`]);
}
