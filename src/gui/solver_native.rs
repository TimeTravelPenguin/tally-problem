use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use iced::Task;
use iced::futures::{StreamExt, channel::mpsc, stream};
use tally_problem::{SearchProgress, SearchSession, TallyCounter};

use super::Update;

const SEARCH_BATCH: usize = 256;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(50);

pub struct Control {
    cancelled: Arc<AtomicBool>,
}

impl Drop for Control {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

pub fn start(counter: TallyCounter, target: Vec<u8>) -> (Control, Task<Result<Update, String>>) {
    let cancelled = Arc::new(AtomicBool::new(false));
    let control = Control {
        cancelled: Arc::clone(&cancelled),
    };

    let task = Task::stream(
        stream::once(async move {
            let (sender, receiver) = mpsc::unbounded();
            let startup_sender = sender.clone();
            let panic_sender = sender.clone();
            let thread = std::thread::Builder::new()
                .name("tally-solver".to_owned())
                .spawn(move || {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        run_search(counter, target, cancelled, sender, PROGRESS_INTERVAL);
                    }));

                    if result.is_err() {
                        let _ = panic_sender.unbounded_send(Err(
                            "The background solver stopped unexpectedly.".to_owned(),
                        ));
                    }
                });

            if let Err(error) = thread {
                let _ = startup_sender.unbounded_send(Err(format!(
                    "Unable to start the background solver: {error}"
                )));
            }

            receiver
        })
        .flatten(),
    );

    (control, task)
}

fn run_search(
    counter: TallyCounter,
    target: Vec<u8>,
    cancelled: Arc<AtomicBool>,
    sender: mpsc::UnboundedSender<Result<Update, String>>,
    progress_interval: Duration,
) {
    if cancelled.load(Ordering::Relaxed) || sender.is_closed() {
        return;
    }

    let started_at = Instant::now();
    let mut session = match SearchSession::new(&counter, &target) {
        Ok(session) => session,
        Err(error) => {
            let _ = sender.unbounded_send(Err(error.to_string()));

            return;
        }
    };

    let mut last_progress = started_at;

    while !cancelled.load(Ordering::Relaxed) && !sender.is_closed() {
        let progress = session
            .advance(SEARCH_BATCH)
            .map_err(|error| error.to_string());
        let complete = !matches!(progress, Ok(SearchProgress::InProgress { .. }));

        if complete || last_progress.elapsed() >= progress_interval {
            let update = progress.map(|progress| Update {
                progress,
                statistics: session.statistics(),
                elapsed: started_at.elapsed(),
            });

            if sender.unbounded_send(update).is_err() {
                break;
            }

            last_progress = Instant::now();
        }

        if complete {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::futures::executor::block_on;
    use tally_problem::SearchResult;

    #[test]
    fn background_search_reports_completion_and_closes_stream() {
        let counter = TallyCounter::new(2).unwrap();
        let (sender, receiver) = mpsc::unbounded();
        let cancelled = Arc::new(AtomicBool::new(false));
        let thread = std::thread::spawn(move || {
            run_search(counter, vec![0, 1], cancelled, sender, PROGRESS_INTERVAL);
        });
        let updates = block_on(receiver.collect::<Vec<_>>());

        thread.join().unwrap();

        assert!(matches!(
            updates.last(),
            Some(Ok(Update {
                progress: SearchProgress::Complete(SearchResult::Found(_)),
                ..
            }))
        ));

        let update = updates.last().unwrap().as_ref().unwrap();

        assert!(update.elapsed > Duration::ZERO);
        assert_eq!(update.statistics.visited_groups, 0);
    }

    #[test]
    fn background_search_pairs_statistics_with_monotonic_elapsed_time() {
        let counter = TallyCounter::new(3).unwrap();
        let (sender, receiver) = mpsc::unbounded();
        let cancelled = Arc::new(AtomicBool::new(false));
        let thread = std::thread::spawn(move || {
            run_search(counter, vec![9, 8, 7], cancelled, sender, Duration::ZERO);
        });
        let updates = block_on(receiver.collect::<Vec<_>>());

        thread.join().unwrap();

        let updates: Vec<_> = updates.into_iter().map(Result::unwrap).collect();

        assert!(updates.len() >= 2);
        assert!(updates.last().unwrap().statistics.visited_groups > 0);
        assert!(updates.last().unwrap().statistics.diagram_work > 0);

        for pair in updates.windows(2) {
            assert!(pair[0].elapsed <= pair[1].elapsed);
            assert!(pair[0].statistics.visited_groups <= pair[1].statistics.visited_groups);
            assert!(pair[0].statistics.diagram_work <= pair[1].statistics.diagram_work);
        }

        for update in updates {
            if let SearchProgress::InProgress { visited_states } = update.progress {
                assert_eq!(visited_states, update.statistics.visited_groups);
            }
        }
    }

    #[test]
    fn dropping_control_cancels_before_search_allocation() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let control = Control {
            cancelled: Arc::clone(&cancelled),
        };

        drop(control);

        let (sender, receiver) = mpsc::unbounded();
        run_search(
            TallyCounter::new(40).unwrap(),
            vec![1; 40],
            cancelled,
            sender,
            PROGRESS_INTERVAL,
        );
        let updates = block_on(receiver.collect::<Vec<_>>());

        assert!(updates.is_empty());
    }

    #[test]
    fn background_search_reports_input_errors() {
        let (sender, receiver) = mpsc::unbounded();
        run_search(
            TallyCounter::new(2).unwrap(),
            vec![1],
            Arc::new(AtomicBool::new(false)),
            sender,
            PROGRESS_INTERVAL,
        );
        let updates = block_on(receiver.collect::<Vec<_>>());

        assert_eq!(updates.len(), 1);
        assert!(updates[0].as_ref().unwrap_err().contains("target length"));
    }
}
