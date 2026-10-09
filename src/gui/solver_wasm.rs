use std::cell::RefCell;
use std::rc::Rc;

use iced::Task;
use iced::futures::channel::mpsc;
use js_sys::{Array, Uint8Array};
use tally_problem::{Action, SearchProgress, SearchResult, TallyCounter};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use web_sys::{ErrorEvent, MessageEvent, Url, Worker};

type Update = Result<SearchProgress, String>;
type Sender = Rc<RefCell<Option<mpsc::UnboundedSender<Update>>>>;

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

pub fn start(counter: TallyCounter, target: Vec<u8>) -> (Control, Task<Update>) {
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

fn publish(sender: &Sender, update: Update) {
    let complete = !matches!(update, Ok(SearchProgress::InProgress { .. }));

    if complete {
        if let Some(sender) = sender.borrow_mut().take() {
            let _ = sender.unbounded_send(update);
        }
    } else if let Some(sender) = sender.borrow().as_ref() {
        let _ = sender.unbounded_send(update);
    }
}

fn decode_update(data: JsValue) -> Update {
    if !Array::is_array(&data) {
        return Err("The background solver returned an invalid response.".to_owned());
    }

    let response = Array::from(&data);

    match response.get(0).as_string().as_deref() {
        Some("progress") => {
            let visited_states = response
                .get(1)
                .as_f64()
                .filter(|count| count.is_finite() && *count >= 0.0)
                .ok_or_else(|| "The background solver returned invalid progress.".to_owned())?
                as usize;

            Ok(SearchProgress::InProgress { visited_states })
        }

        Some("found") => {
            let encoded = response.get(1);

            if !Array::is_array(&encoded) {
                return Err("The background solver returned invalid actions.".to_owned());
            }

            let mut actions = Vec::new();

            for encoded_action in Array::from(&encoded).iter() {
                if !Array::is_array(&encoded_action) {
                    return Err("The background solver returned an invalid action.".to_owned());
                }

                let encoded_action = Array::from(&encoded_action);
                let ticks = encoded_action
                    .get(1)
                    .as_string()
                    .and_then(|ticks| ticks.parse::<u64>().ok())
                    .ok_or_else(|| "The background solver returned invalid ticks.".to_owned())?;

                let action = match encoded_action.get(0).as_string().as_deref() {
                    Some("increment") => Action::Increment(ticks),
                    Some("forward") => Action::ResetForward(ticks),
                    Some("backward") => Action::ResetBackward(ticks),
                    _ => return Err("The background solver returned an unknown action.".to_owned()),
                };

                actions.push(action);
            }

            Ok(SearchProgress::Complete(SearchResult::Found(actions)))
        }

        Some("not-found") => Ok(SearchProgress::Complete(SearchResult::NotFound)),
        Some("error") => Err(response
            .get(1)
            .as_string()
            .unwrap_or_else(|| "The background solver failed.".to_owned())),
        _ => Err("The background solver returned an unknown response.".to_owned()),
    }
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
