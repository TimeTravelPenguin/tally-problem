use std::{collections::VecDeque, time::Duration};

use tally_problem::{SearchStatistics, TallyCounter, increment_lower_bound};

const SAMPLE_INTERVAL: Duration = Duration::from_millis(50);
const WARMUP: Duration = Duration::from_millis(250);
const SAMPLE_COUNT: usize = 8;

#[derive(Debug, Clone, Copy)]
pub(crate) struct TimingPrediction {
    pub(crate) percent: f32,
    pub(crate) remaining: Duration,
}

/// A deliberately limited work prediction, calibrated against release searches.
///
/// Shared diagram operations vary greatly in cost per state group. Instead of
/// treating every group equally, use observed diagram work and worker timing.
/// The input provides a rough total-work prior; measurements can revise it or
/// withdraw the prediction when the search no longer fits the calibrated cases.
#[derive(Debug, Clone)]
pub(crate) struct ProgressEstimate {
    minimum_increments: Option<u64>,
    initial_work: Option<f64>,
    total_work: Option<f64>,
    latest: SearchStatistics,
    elapsed: Duration,
    samples: VecDeque<Sample>,
    exceeded: bool,
}

#[derive(Debug, Clone, Copy)]
struct Sample {
    elapsed: Duration,
    work: u64,
    groups: usize,
}

impl ProgressEstimate {
    pub(crate) fn new(counter: &TallyCounter, target: &[u8]) -> Self {
        let initial_work = estimate_work(counter.values(), target);

        Self {
            minimum_increments: increment_lower_bound(counter, target).ok(),
            initial_work,
            total_work: initial_work,
            latest: SearchStatistics::default(),
            elapsed: Duration::ZERO,
            samples: VecDeque::from([Sample {
                elapsed: Duration::ZERO,
                work: 0,
                groups: 0,
            }]),
            exceeded: false,
        }
    }

    /// Guaranteed minimum increments, independent of timing samples or estimates.
    pub(crate) fn minimum_increments(&self) -> Option<u64> {
        self.minimum_increments
    }

    pub(crate) fn update(&mut self, statistics: SearchStatistics, elapsed: Duration) {
        if elapsed < self.elapsed || statistics.diagram_work < self.latest.diagram_work {
            return;
        }

        self.latest = statistics;
        self.elapsed = elapsed;

        if elapsed.saturating_sub(self.samples.back().unwrap().elapsed) >= SAMPLE_INTERVAL {
            self.samples.push_back(Sample {
                elapsed,
                work: statistics.diagram_work,
                groups: statistics.visited_groups,
            });

            if self.samples.len() > SAMPLE_COUNT {
                self.samples.pop_front();
            }
        }

        let Some(initial_work) = self.initial_work else {
            return;
        };

        let observed_work = statistics.diagram_work as f64;

        if observed_work >= initial_work {
            // Do not invent a diminishing tail after the prior has run out.
            self.exceeded = true;
            self.total_work = None;
            return;
        }

        if let Some(first) = self.samples.front() {
            let groups = statistics.visited_groups.saturating_sub(first.groups);
            let work = statistics.diagram_work.saturating_sub(first.work);

            if groups > 0 {
                // Allow for another queued wave at the recently observed cost.
                // This is a revision, not an assertion that the queue is the
                // entire remaining search. Large revisions invalidate the prior.
                let queued_work =
                    work as f64 / groups as f64 * statistics.queued_groups as f64 * 2.0;
                let revised_work = initial_work.max(observed_work + queued_work);

                self.total_work = (revised_work <= initial_work * 1.4).then_some(revised_work);
            }
        }
    }

    pub(crate) fn prediction(&self) -> Option<TimingPrediction> {
        if self.exceeded
            || self.elapsed < WARMUP
            || self.latest.visited_groups < 128
            || self.latest.diagram_work < 100_000
            || self.samples.len() < 4
        {
            return None;
        }

        let total_work = self.total_work?;
        let observed_work = self.latest.diagram_work as f64;
        let mut rates = Vec::with_capacity(SAMPLE_COUNT - 1);

        for (previous, current) in self.samples.iter().zip(self.samples.iter().skip(1)) {
            let duration = current
                .elapsed
                .saturating_sub(previous.elapsed)
                .as_secs_f64();
            let work = current.work.saturating_sub(previous.work);

            if duration > 0.0 && work > 0 {
                rates.push(work as f64 / duration);
            }
        }

        if rates.len() < 3 {
            return None;
        }

        rates.sort_by(f64::total_cmp);
        let median_rate = rates[rates.len() / 2];
        let overall_rate = observed_work / self.elapsed.as_secs_f64();
        let lowest_rate = rates[0];
        let highest_rate = rates[rates.len() - 1];

        if !median_rate.is_finite()
            || median_rate <= 0.0
            || highest_rate > lowest_rate * 4.0
            || median_rate < overall_rate * 0.4
            || median_rate > overall_rate * 2.5
        {
            return None;
        }

        // A recent slowdown changes the ETA immediately; short cheap batches
        // cannot make the remaining expensive work appear nearly instantaneous.
        let rate = median_rate.min(overall_rate * 1.1);
        let remaining_seconds = (total_work - observed_work) / rate;

        if !remaining_seconds.is_finite() || !(0.0..=86_400.0).contains(&remaining_seconds) {
            return None;
        }

        Some(TimingPrediction {
            percent: (self.elapsed.as_secs_f64() / (self.elapsed.as_secs_f64() + remaining_seconds)
                * 100.0)
                .clamp(0.0, 99.0) as f32,
            remaining: Duration::from_secs_f64(remaining_seconds),
        })
    }
}

fn estimate_work(initial: &[u8], target: &[u8]) -> Option<f64> {
    // Calibration covers one through seven wheels, with equal starting digits.
    // Unknown patterns should get an activity indicator rather than a percentage.
    if initial.len() != target.len()
        || !(1..=7).contains(&target.len())
        || target.iter().any(|&digit| digit > 9)
        || !initial.iter().all(|digit| Some(digit) == initial.first())
        || initial == target
    {
        return None;
    }

    let mut digit_mask = 0_u16;
    let mut effective_width = 0_usize;
    let mut previous_digit = None;
    let mut run_length = 0_usize;

    for &digit in target {
        digit_mask |= 1 << digit;
        run_length = if previous_digit == Some(digit) {
            run_length + 1
        } else {
            1
        };

        // Long runs share diagram branches. Two representatives retain their
        // extra cost without assigning exponential work to every leading zero.
        if run_length <= 2 {
            effective_width += 1;
        }

        previous_digit = Some(digit);
    }

    let diversity = digit_mask.count_ones();

    if diversity <= 1 {
        return None;
    }

    let boundaries = target
        .windows(2)
        .filter(|digits| digits[0] != digits[1])
        .count();

    if boundaries == 1 && effective_width <= 3 {
        return Some(30_000.0 + 50_000.0 * target.len().saturating_sub(2) as f64);
    }

    // Rounded total diagram-work scales observed across varied, alternating,
    // ascending, and descending patterns. The final scale extrapolates the
    // roughly sixfold increase from five to six wheels; seven-wheel cases were
    // held out when checking the estimate, including 9876543 and 9090909.
    let base_work = match effective_width {
        2 => 20_000.0,
        3 => 190_000.0,
        4 => 1_650_000.0,
        5 => 10_200_000.0,
        6 => 61_000_000.0,
        7 => 366_000_000.0,
        _ => return None,
    };

    if diversity == 2 {
        return Some(base_work * 0.65);
    }

    // Several adjacent repeated runs have not shown a reliable work scale in
    // the bounded cases. Their shared structure can dominate the timing.
    if boundaries + 1 != target.len() {
        return None;
    }

    let ascending = target
        .windows(2)
        .filter(|digits| digits[0] < digits[1])
        .count();
    let ascending_fraction = ascending as f64 / boundaries as f64;
    let growth = if ascending_fraction >= 0.75 {
        1.0 + 0.3 * effective_width.saturating_sub(4) as f64
    } else {
        1.0
    };

    Some(base_work * growth)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn statistics(groups: usize, work: u64, queue: usize) -> SearchStatistics {
        SearchStatistics {
            visited_groups: groups,
            diagram_work: work,
            queued_groups: queue,
            ..SearchStatistics::default()
        }
    }

    fn warmed_estimate() -> ProgressEstimate {
        let counter = TallyCounter::new(7).unwrap();
        let mut estimate = ProgressEstimate::new(&counter, &[9, 8, 7, 6, 5, 4, 3]);

        for idx in 1..=6 {
            estimate.update(
                statistics(idx * 100, idx as u64 * 5_000_000, 20),
                Duration::from_millis(idx as u64 * 50),
            );
        }

        estimate
    }

    #[test]
    fn timing_uses_measured_work_and_a_new_run_starts_without_a_prediction() {
        let estimate = warmed_estimate();
        let prediction = estimate.prediction().unwrap();
        assert!((prediction.percent - 8.2).abs() < 0.1);
        assert!((prediction.remaining.as_secs_f64() - 3.36).abs() < 0.01);

        let counter = TallyCounter::new(7).unwrap();
        let restarted = ProgressEstimate::new(&counter, &[9, 8, 7, 6, 5, 4, 3]);
        assert!(restarted.prediction().is_none());
    }

    #[test]
    fn mathematical_bounds_remain_available_without_a_timing_prediction() {
        let counter = TallyCounter::new(12).unwrap();
        let estimate = ProgressEstimate::new(&counter, &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 9, 9]);
        assert_eq!(estimate.minimum_increments(), Some(143));
        assert!(estimate.prediction().is_none());
    }

    #[test]
    fn recorded_seven_wheel_search_does_not_claim_ninety_percent_early() {
        // Recorded release checkpoints for 0000000 -> 9876543 (6.62 s total).
        // At this point the previous group-only estimate was approaching 90%.
        let counter = TallyCounter::new(7).unwrap();
        let mut estimate = ProgressEstimate::new(&counter, &[9, 8, 7, 6, 5, 4, 3]);
        let checkpoints = [
            (908_201, 22_837, 63_486_075, 1_718),
            (962_781, 23_285, 66_760_106, 1_718),
            (1_014_176, 23_605, 69_747_236, 1_718),
            (1_069_674, 23_861, 72_314_932, 1_718),
            (1_123_775, 24_527, 74_708_801, 1_559),
            (1_197_992, 25_039, 81_062_249, 1_559),
            (1_252_061, 25_231, 83_519_822, 1_559),
            (1_302_762, 25_487, 86_174_793, 1_559),
        ];

        for (microseconds, groups, work, queue) in checkpoints {
            estimate.update(
                statistics(groups, work, queue),
                Duration::from_micros(microseconds),
            );
        }

        let prediction = estimate.prediction().unwrap();
        let actual_remaining = 6.622_124 - 1.302_762;
        assert!((10.0..30.0).contains(&prediction.percent));
        assert!(
            (actual_remaining * 0.7..actual_remaining * 1.5)
                .contains(&prediction.remaining.as_secs_f64())
        );
    }

    #[test]
    fn exceeded_priors_and_uncalibrated_patterns_have_no_percentage_tail() {
        let mut estimate = warmed_estimate();
        estimate.update(statistics(1_000, 367_000_000, 20), Duration::from_secs(4));
        assert!(estimate.prediction().is_none());

        let mut mixed = TallyCounter::new(7).unwrap();
        mixed.set_values(vec![1, 2, 3, 4, 5, 6, 7]).unwrap();
        let unknown = ProgressEstimate::new(&mixed, &[7, 6, 5, 4, 3, 2, 1]);
        assert!(unknown.initial_work.is_none());

        let wide = TallyCounter::new(1_000).unwrap();
        assert!(
            ProgressEstimate::new(&wide, &vec![5; 1_000])
                .prediction()
                .is_none()
        );
    }

    #[test]
    fn queued_work_can_revise_the_total_and_percentage_backwards() {
        let mut estimate = warmed_estimate();
        let earlier = estimate.prediction().unwrap().percent;
        estimate.update(
            statistics(620, 31_000_000, 4_000),
            Duration::from_millis(310),
        );
        let revised = estimate.prediction().unwrap().percent;
        assert!(revised < earlier);
    }

    #[test]
    fn a_moderate_slowdown_revises_both_time_and_percentage() {
        let mut estimate = warmed_estimate();
        let earlier = estimate.prediction().unwrap();

        for idx in 1..=4 {
            estimate.update(
                statistics(600 + idx * 100, 30_000_000 + idx as u64 * 2_500_000, 20),
                Duration::from_millis(300 + idx as u64 * 50),
            );
        }

        let slower = estimate.prediction().unwrap();
        assert!(slower.remaining > earlier.remaining);
        assert!(slower.percent < earlier.percent);
    }

    #[test]
    fn unstable_or_stale_observations_do_not_produce_confident_timings() {
        let mut estimate = warmed_estimate();
        let earlier = estimate.prediction().unwrap();
        estimate.update(statistics(1, 1, 1), Duration::from_millis(1));
        assert_eq!(estimate.prediction().unwrap().percent, earlier.percent);

        estimate.update(statistics(700, 31_000_000, 20), Duration::from_secs(3));
        assert!(estimate.prediction().is_none());
        estimate.update(statistics(usize::MAX, u64::MAX, usize::MAX), Duration::MAX);
        assert!(estimate.prediction().is_none());
    }
}
