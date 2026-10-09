use js_sys::{Array, Uint8Array};
use tally_problem::{Action, SearchProgress, SearchResult, SearchSession, TallyCounter};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use web_sys::{DedicatedWorkerGlobalScope, MessageEvent};

const SEARCH_BATCH: usize = 256;
const PROGRESS_INTERVAL_MS: f64 = 50.0;

pub fn install() {
    let scope: DedicatedWorkerGlobalScope = js_sys::global().unchecked_into();
    let message_scope = scope.clone();
    let message = Closure::wrap(Box::new(move |event: MessageEvent| {
        if let Err(error) = run_search(&message_scope, event.data()) {
            let response = Array::new();
            response.push(&JsValue::from_str("error"));
            response.push(&JsValue::from_str(&error));
            let _ = message_scope.post_message(&response);
        }
    }) as Box<dyn FnMut(MessageEvent)>);

    scope.set_onmessage(Some(message.as_ref().unchecked_ref()));
    // The worker lives until the UI terminates it; its callback shares that lifetime.
    message.forget();

    let ready = Array::new();
    ready.push(&JsValue::from_str("ready"));
    let _ = scope.post_message(&ready);
}

fn run_search(scope: &DedicatedWorkerGlobalScope, data: JsValue) -> Result<(), String> {
    if !Array::is_array(&data) {
        return Err("The background solver received an invalid request.".to_owned());
    }

    let request = Array::from(&data);
    let values = request
        .get(0)
        .dyn_into::<Uint8Array>()
        .map_err(|_| "The background solver received invalid starting digits.".to_owned())?
        .to_vec();
    let target = request
        .get(1)
        .dyn_into::<Uint8Array>()
        .map_err(|_| "The background solver received invalid target digits.".to_owned())?
        .to_vec();
    let reset_index = request
        .get(2)
        .as_f64()
        .filter(|index| index.is_finite() && index.fract() == 0.0 && (0.0..=9.0).contains(index))
        .ok_or_else(|| "The background solver received an invalid reset index.".to_owned())?
        as u8;

    let mut counter = TallyCounter::new(values.len()).map_err(|error| error.to_string())?;
    counter
        .set_values(values)
        .map_err(|error| error.to_string())?;
    counter
        .set_reset_index(reset_index)
        .map_err(|error| error.to_string())?;

    let mut session = SearchSession::new(&counter, &target).map_err(|error| error.to_string())?;
    let mut last_progress = js_sys::Date::now();

    loop {
        let progress = session
            .advance(SEARCH_BATCH)
            .map_err(|error| error.to_string())?;
        let complete = matches!(progress, SearchProgress::Complete(_));
        let now = js_sys::Date::now();

        if complete || now - last_progress >= PROGRESS_INTERVAL_MS {
            scope
                .post_message(&encode_progress(progress))
                .map_err(|_| "Unable to deliver the background solver's response.".to_owned())?;
            last_progress = now;
        }

        if complete {
            break;
        }
    }

    Ok(())
}

fn encode_progress(progress: SearchProgress) -> Array {
    let response = Array::new();

    match progress {
        SearchProgress::InProgress { visited_states } => {
            response.push(&JsValue::from_str("progress"));
            response.push(&JsValue::from_f64(visited_states as f64));
        }

        SearchProgress::Complete(SearchResult::NotFound) => {
            response.push(&JsValue::from_str("not-found"));
        }

        SearchProgress::Complete(SearchResult::Found(actions)) => {
            let encoded = Array::new();

            for action in actions {
                let (kind, ticks) = match action {
                    Action::Increment(ticks) => ("increment", ticks),
                    Action::ResetForward(ticks) => ("forward", ticks),
                    Action::ResetBackward(ticks) => ("backward", ticks),
                };

                let encoded_action = Array::new();
                encoded_action.push(&JsValue::from_str(kind));
                // JavaScript numbers cannot represent every u64 exactly.
                encoded_action.push(&JsValue::from_str(&ticks.to_string()));
                encoded.push(&encoded_action);
            }

            response.push(&JsValue::from_str("found"));
            response.push(&encoded);
        }
    }

    response
}
