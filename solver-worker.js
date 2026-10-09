/**
 * @file Load Trunk's stable worker bindings and WASM beside this bootstrap.
 * Rust announces `ready` after installing its handler; the GUI waits for that
 * message before sending a search. Loading failures use an unversioned error
 * response because the Rust worker protocol is not installed yet.
 */
try {
  importScripts("tally-worker.js");
  wasm_bindgen("tally-worker_bg.wasm").catch((error) => {
    self.postMessage(["error", `Unable to load the background solver: ${error}`]);
  });
} catch (error) {
  self.postMessage(["error", `Unable to load the background solver: ${error}`]);
}
