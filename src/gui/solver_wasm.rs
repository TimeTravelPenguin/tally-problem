//! Browser transport for a dedicated solver Web Worker.
//!
//! A new worker is created for each job. Its `ready` handshake precedes the
//! `search-v1` request, and `update-v1` responses become Iced task updates.
//! The worker measures elapsed time after receiving the request, so module
//! loading and message delivery do not contribute to reported solver time.
//!
//! Counts use validated JavaScript integers; action ticks and other `u64`
//! counters use decimal strings to preserve values beyond JavaScript's exact
//! numeric range. Dropping [`Control`] terminates the worker and closes its
//! update stream, including during startup.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use iced::Task;
use iced::futures::channel::mpsc;
use js_sys::{Array, Uint8Array};
use tally_problem::{Action, SearchProgress, SearchResult, SearchStatistics, TallyCounter};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use web_sys::{ErrorEvent, MessageEvent, Url, Worker};

use super::Update;

type Response = Result<Update, String>;
type Sender = Rc<RefCell<Option<mpsc::UnboundedSender<Response>>>>;

const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// Owns the worker and its callbacks. Dropping a job immediately stops its CPU
/// work and releases the worker's WASM memory, even during initialization.
pub struct Control {
    worker: Option<Worker>,
    sender: Sender,
    _message: Option<Closure<dyn FnMut(MessageEvent)>>,
    _error: Option<Closure<dyn FnMut(ErrorEvent)>>,
    _message_error: Option<Closure<dyn FnMut(MessageEvent)>>,
}

impl Drop for Control {
    fn drop(&mut self) {
        if let Some(worker) = &self.worker {
            worker.set_onmessage(None);
            worker.set_onerror(None);
            worker.set_onmessageerror(None);
            worker.terminate();
        }

        self.sender.borrow_mut().take();
    }
}

/// Create a worker and return its lifetime handle and decoded update stream.
///
/// The request is sent once, after Rust has installed its worker-side handler.
/// Startup and protocol failures are surfaced as terminal stream errors.
pub fn start(counter: TallyCounter, target: Vec<u8>) -> (Control, Task<Response>) {
    let (sender, receiver) = mpsc::unbounded();
    let sender = Rc::new(RefCell::new(Some(sender)));
    let mut control = Control {
        worker: None,
        sender: Rc::clone(&sender),
        _message: None,
        _error: None,
        _message_error: None,
    };

    let worker = match create_worker() {
        Ok(worker) => worker,
        Err(error) => {
            publish(&sender, Err(error));

            return (control, Task::stream(receiver));
        }
    };

    let request = Array::new();
    request.push(&JsValue::from_str("search-v1"));
    request.push(&Uint8Array::from(counter.values()));
    request.push(&Uint8Array::from(target.as_slice()));
    request.push(&JsValue::from(counter.reset_index()));

    let message_sender = Rc::clone(&sender);
    let message_worker = worker.clone();
    let mut request = Some(request);
    let message = Closure::wrap(Box::new(move |event: MessageEvent| {
        let data = event.data();

        if Array::is_array(&data)
            && Array::from(&data).get(0).as_string().as_deref() == Some("ready")
        {
            if let Some(request) = request.take()
                && let Err(error) = message_worker.post_message(&request)
            {
                publish(
                    &message_sender,
                    Err(format!("Unable to start the search: {}", js_error(error))),
                );
            }

            return;
        }

        publish(&message_sender, decode_update(data));
    }) as Box<dyn FnMut(MessageEvent)>);

    let error_sender = Rc::clone(&sender);
    let error = Closure::wrap(Box::new(move |event: ErrorEvent| {
        publish(
            &error_sender,
            Err(format!(
                "The background solver stopped unexpectedly: {}",
                event.message()
            )),
        );
    }) as Box<dyn FnMut(ErrorEvent)>);

    let message_error_sender = Rc::clone(&sender);
    let message_error = Closure::wrap(Box::new(move |_event: MessageEvent| {
        publish(
            &message_error_sender,
            Err("Unable to read the background solver's response.".to_owned()),
        );
    }) as Box<dyn FnMut(MessageEvent)>);

    worker.set_onmessage(Some(message.as_ref().unchecked_ref()));
    worker.set_onerror(Some(error.as_ref().unchecked_ref()));
    worker.set_onmessageerror(Some(message_error.as_ref().unchecked_ref()));
    control.worker = Some(worker);
    control._message = Some(message);
    control._error = Some(error);
    control._message_error = Some(message_error);

    (control, Task::stream(receiver))
}

/// Resolve the bootstrap against the document base URL, including Pages subpaths.
fn create_worker() -> Result<Worker, String> {
    let base = web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| "The browser document is unavailable.".to_owned())?
        .base_uri()
        .map_err(js_error)?
        .ok_or_else(|| "The browser's base URL is unavailable.".to_owned())?;

    let url = Url::new_with_base("solver-worker.js", &base).map_err(js_error)?;

    Worker::new(&url.href())
        .map_err(|error| format!("Unable to start the background solver: {}", js_error(error)))
}

/// Forward progress while closing the stream after its first terminal update.
fn publish(sender: &Sender, update: Response) {
    let complete = !matches!(
        update,
        Ok(Update {
            progress: SearchProgress::InProgress { .. },
            ..
        })
    );

    if complete {
        if let Some(sender) = sender.borrow_mut().take() {
            let _ = sender.unbounded_send(update);
        }
    } else if let Some(sender) = sender.borrow().as_ref() {
        let _ = sender.unbounded_send(update);
    }
}

/// Validate the versioned response and pair its result with worker telemetry.
///
/// Bootstrap errors are also accepted because WASM can fail before the worker
/// installs its versioned protocol. In-progress counts must agree with the
/// accompanying statistics snapshot.
fn decode_update(data: JsValue) -> Response {
    if !Array::is_array(&data) {
        return Err("The background solver returned an invalid response.".to_owned());
    }

    let response = Array::from(&data);

    match response.get(0).as_string().as_deref() {
        // The bootstrap can fail before Rust installs the versioned protocol.
        Some("error" | "error-v1") => {
            return Err(response
                .get(1)
                .as_string()
                .unwrap_or_else(|| "The background solver failed.".to_owned()));
        }

        Some("update-v1") if response.length() == 5 => {}
        _ => return Err("The background solver returned an unknown response.".to_owned()),
    }

    let statistics = decode_statistics(response.get(3))?;
    let elapsed_ms = response
        .get(4)
        .as_f64()
        .filter(|elapsed| elapsed.is_finite() && (0.0..=MAX_SAFE_INTEGER).contains(elapsed))
        .ok_or_else(|| "The background solver returned invalid timing.".to_owned())?;
    let elapsed = Duration::try_from_secs_f64(elapsed_ms / 1000.0)
        .map_err(|_| "The background solver returned invalid timing.".to_owned())?;

    let progress = match response.get(1).as_string().as_deref() {
        Some("progress") => {
            let visited_states = decode_count(response.get(2))?;

            if visited_states != statistics.visited_groups {
                return Err("The background solver returned inconsistent progress.".to_owned());
            }

            SearchProgress::InProgress { visited_states }
        }

        Some("found") => {
            let encoded = response.get(2);

            if !Array::is_array(&encoded) {
                return Err("The background solver returned invalid actions.".to_owned());
            }

            let mut actions = Vec::new();

            for encoded_action in Array::from(&encoded).iter() {
                if !Array::is_array(&encoded_action) {
                    return Err("The background solver returned an invalid action.".to_owned());
                }

                let encoded_action = Array::from(&encoded_action);

                if encoded_action.length() != 2 {
                    return Err("The background solver returned an invalid action.".to_owned());
                }

                let ticks = decode_u64(encoded_action.get(1))?;

                let action = match encoded_action.get(0).as_string().as_deref() {
                    Some("increment") => Action::Increment(ticks),
                    Some("forward") => Action::ResetForward(ticks),
                    Some("backward") => Action::ResetBackward(ticks),
                    _ => return Err("The background solver returned an unknown action.".to_owned()),
                };

                actions.push(action);
            }

            SearchProgress::Complete(SearchResult::Found(actions))
        }

        Some("not-found") => SearchProgress::Complete(SearchResult::NotFound),
        _ => return Err("The background solver returned an unknown response.".to_owned()),
    };

    Ok(Update {
        progress,
        statistics,
        elapsed,
    })
}

/// Decode the fixed field order shared with the worker's response encoder.
fn decode_statistics(data: JsValue) -> Result<SearchStatistics, String> {
    if !Array::is_array(&data) {
        return Err("The background solver returned invalid statistics.".to_owned());
    }

    let statistics = Array::from(&data);

    if statistics.length() != 6 {
        return Err("The background solver returned invalid statistics.".to_owned());
    }

    Ok(SearchStatistics {
        visited_groups: decode_count(statistics.get(0))?,
        increment_layer: decode_u64(statistics.get(1))?,
        reset_ticks: decode_u64(statistics.get(2))?,
        diagram_nodes: decode_count(statistics.get(3))?,
        diagram_work: decode_u64(statistics.get(4))?,
        queued_groups: decode_count(statistics.get(5))?,
    })
}

/// Accept only exact, nonnegative JavaScript integers that also fit `usize`.
fn decode_count(value: JsValue) -> Result<usize, String> {
    value
        .as_f64()
        .filter(|count| {
            count.is_finite()
                && count.fract() == 0.0
                && (0.0..=MAX_SAFE_INTEGER.min(usize::MAX as f64)).contains(count)
        })
        .map(|count| count as usize)
        .ok_or_else(|| "The background solver returned an invalid group count.".to_owned())
}

/// Decode decimal strings without passing exact Rust counters through `f64`.
fn decode_u64(value: JsValue) -> Result<u64, String> {
    value
        .as_string()
        .filter(|digits| !digits.is_empty() && digits.bytes().all(|digit| digit.is_ascii_digit()))
        .and_then(|digits| digits.parse().ok())
        .ok_or_else(|| "The background solver returned an invalid integer.".to_owned())
}

fn js_error(error: JsValue) -> String {
    error
        .as_string()
        .or_else(|| {
            js_sys::Reflect::get(&error, &JsValue::from_str("message"))
                .ok()?
                .as_string()
        })
        .unwrap_or_else(|| "Browser operation failed.".to_owned())
}
