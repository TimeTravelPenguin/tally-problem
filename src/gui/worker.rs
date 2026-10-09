use js_sys::{Array, Uint8Array};
use tally_problem::{
    Action, SearchProgress, SearchResult, SearchSession, SearchStatistics, TallyCounter,
};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use web_sys::{DedicatedWorkerGlobalScope, MessageEvent, Performance};

const SEARCH_BATCH: usize = 256;
const PROGRESS_INTERVAL_MS: f64 = 50.0;
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// Worker-local monotonic elapsed time, including counter and solver setup.
/// Older browsers without Performance use Date, clamped against clock changes.
struct SearchClock {
    performance: Option<Performance>,
    started_at: Option<f64>,
    elapsed_ms: f64,
}

impl SearchClock {
    fn new(scope: &DedicatedWorkerGlobalScope) -> Self {
        let performance = scope
            .performance()
            .filter(|performance| performance.now().is_finite());
        let started_at = performance
            .as_ref()
            .map_or_else(js_sys::Date::now, Performance::now);

        Self {
            performance,
            started_at: started_at.is_finite().then_some(started_at),
            elapsed_ms: 0.0,
        }
    }

    fn elapsed_ms(&mut self) -> f64 {
        let now = self
            .performance
            .as_ref()
            .map_or_else(js_sys::Date::now, Performance::now);
        if !now.is_finite() {
            return self.elapsed_ms;
        }

        let started_at = self.started_at.get_or_insert(now);
        let elapsed_ms = now - *started_at;

        if elapsed_ms.is_finite() {
            self.elapsed_ms = self.elapsed_ms.max(elapsed_ms).min(MAX_SAFE_INTEGER);
        }

        self.elapsed_ms
    }
}

pub fn install() {
    let scope: DedicatedWorkerGlobalScope = js_sys::global().unchecked_into();
    let message_scope = scope.clone();
    let message = Closure::wrap(Box::new(move |event: MessageEvent| {
        if let Err(error) = run_search(&message_scope, event.data()) {
            let response = Array::new();
            response.push(&JsValue::from_str("error-v1"));
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
    let mut clock = SearchClock::new(scope);

    if !Array::is_array(&data) {
        return Err("The background solver received an invalid request.".to_owned());
    }

    let request = Array::from(&data);

    if request.length() != 4 || request.get(0).as_string().as_deref() != Some("search-v1") {
        return Err("The background solver received an unknown request.".to_owned());
    }

    let values = request
        .get(1)
        .dyn_into::<Uint8Array>()
        .map_err(|_| "The background solver received invalid starting digits.".to_owned())?
        .to_vec();
    let target = request
        .get(2)
        .dyn_into::<Uint8Array>()
        .map_err(|_| "The background solver received invalid target digits.".to_owned())?
        .to_vec();
    let reset_index = request
        .get(3)
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
    let mut last_progress = 0.0;

    loop {
        let progress = session
            .advance(SEARCH_BATCH)
            .map_err(|error| error.to_string())?;
        let complete = matches!(progress, SearchProgress::Complete(_));
        let elapsed_ms = clock.elapsed_ms();

        if complete || elapsed_ms - last_progress >= PROGRESS_INTERVAL_MS {
            scope
                .post_message(&encode_progress(progress, session.statistics(), elapsed_ms))
                .map_err(|_| "Unable to deliver the background solver's response.".to_owned())?;
            last_progress = elapsed_ms;
        }

        if complete {
            break;
        }
    }

    Ok(())
}

fn encode_progress(
    progress: SearchProgress,
    statistics: SearchStatistics,
    elapsed_ms: f64,
) -> Array {
    let response = Array::new();
    response.push(&JsValue::from_str("update-v1"));

    match progress {
        SearchProgress::InProgress { visited_states } => {
            response.push(&JsValue::from_str("progress"));
            response.push(&JsValue::from_f64(visited_states as f64));
        }

        SearchProgress::Complete(SearchResult::NotFound) => {
            response.push(&JsValue::from_str("not-found"));
            response.push(&JsValue::NULL);
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

    let encoded_statistics = Array::new();
    encoded_statistics.push(&JsValue::from_f64(statistics.visited_groups as f64));
    encoded_statistics.push(&JsValue::from_str(&statistics.increment_layer.to_string()));
    encoded_statistics.push(&JsValue::from_str(&statistics.reset_ticks.to_string()));
    encoded_statistics.push(&JsValue::from_f64(statistics.diagram_nodes as f64));
    encoded_statistics.push(&JsValue::from_str(&statistics.diagram_work.to_string()));
    encoded_statistics.push(&JsValue::from_f64(statistics.queued_groups as f64));
    response.push(&encoded_statistics);
    response.push(&JsValue::from_f64(elapsed_ms));

    response
}
