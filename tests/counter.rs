use tally_problem::{TallyCounter, TallyCounterError};

#[test]
fn zero_digit_counter_is_rejected() {
    assert_eq!(
        TallyCounter::new(0),
        Err(TallyCounterError::InvalidDigitCount(0))
    );
}

#[test]
fn invalid_values_leave_counter_and_hard_reset_width_unchanged() {
    let mut initial = TallyCounter::new(3).unwrap();
    initial.set_values(vec![4, 5, 6]).unwrap();
    initial.set_reset_index(7).unwrap();

    for (invalid_values, expected_error) in [
        (vec![], TallyCounterError::InvalidDigitCount(0)),
        (vec![1, 10], TallyCounterError::InvalidDigitValue(10)),
    ] {
        let mut counter = initial.clone();

        assert_eq!(counter.set_values(invalid_values), Err(expected_error));
        assert_eq!(counter, initial);

        counter.hard_reset();

        assert_eq!(counter.values(), &[0, 0, 0]);
        assert_eq!(counter.digit_count(), 3);
        assert_eq!(counter.reset_index(), 0);
    }
}

#[test]
fn invalid_reset_position_leaves_counter_unchanged() {
    let mut counter = TallyCounter::new(2).unwrap();
    counter.set_values(vec![3, 4]).unwrap();
    counter.set_reset_index(8).unwrap();

    let initial = counter.clone();

    for reset_index in [10, u8::MAX] {
        assert_eq!(
            counter.set_reset_index(reset_index),
            Err(TallyCounterError::InvalidResetIndex(reset_index))
        );
        assert_eq!(counter, initial);
    }
}

#[test]
fn replacing_values_resizes_counter_and_preserves_reset_position() {
    let mut counter = TallyCounter::new(2).unwrap();
    counter.set_reset_index(6).unwrap();
    counter.set_values(vec![0, 1, 2, 3, 4]).unwrap();

    assert_eq!(counter.values(), &[0, 1, 2, 3, 4]);
    assert_eq!(counter.digit_count(), 5);
    assert_eq!(counter.reset_index(), 6);

    counter.hard_reset();

    assert_eq!(counter.values(), &[0, 0, 0, 0, 0]);
    assert_eq!(counter.digit_count(), 5);
    assert_eq!(counter.reset_index(), 0);
}

#[test]
fn increments_match_decimal_arithmetic_including_maximal_amounts() {
    for values in [
        vec![9],
        vec![9, 9],
        vec![0, 0, 9, 9],
        vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 0],
        vec![9; 20],
    ] {
        let width = values.len();
        let initial_value = values
            .iter()
            .fold(0u128, |acc, &digit| acc * 10 + u128::from(digit));
        let modulus = 10u128.pow(width.try_into().unwrap());

        for amount in [0, 1, 9, 10, 99, u64::MAX] {
            let expected_value = (initial_value + u128::from(amount)) % modulus;
            let expected_digits: Vec<u8> = format!("{expected_value:0width$}")
                .bytes()
                .map(|digit| digit - b'0')
                .collect();

            let mut counter = TallyCounter::new(width).unwrap();
            counter.set_values(values.clone()).unwrap();
            counter.set_reset_index(7).unwrap();
            counter.increment_by(amount);

            assert_eq!(counter.values(), expected_digits, "amount={amount}");
            assert_eq!(counter.reset_index(), 7);
        }
    }
}

#[test]
fn forward_reset_batches_match_independent_one_tick_simulation() {
    for values in [vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9], vec![9, 9, 0, 0, 5]] {
        for reset_index in 0..10 {
            for ticks in (0..=25).chain([99, 100, 101]) {
                let mut expected_values = values.clone();
                let mut expected_reset = reset_index;

                for _ in 0..ticks {
                    let next_reset = (expected_reset + 1) % 10;

                    for digit in &mut expected_values {
                        if *digit == expected_reset {
                            *digit = next_reset;
                        }
                    }

                    expected_reset = next_reset;
                }

                let mut counter = TallyCounter::new(values.len()).unwrap();
                counter.set_values(values.clone()).unwrap();
                counter.set_reset_index(reset_index).unwrap();
                counter.reset_forward_by(ticks);

                assert_eq!(
                    counter.values(),
                    expected_values,
                    "reset_index={reset_index}, ticks={ticks}"
                );
                assert_eq!(counter.reset_index(), expected_reset);
            }
        }
    }
}

#[test]
fn backward_reset_batches_match_one_tick_simulation_and_preserve_values() {
    let values = vec![9, 0, 4, 4, 2];

    for reset_index in 0..10 {
        for ticks in (0..=25).chain([99, 100, 101]) {
            let mut expected_reset = reset_index;

            for _ in 0..ticks {
                expected_reset = (expected_reset + 9) % 10;
            }

            let mut counter = TallyCounter::new(values.len()).unwrap();
            counter.set_values(values.clone()).unwrap();
            counter.set_reset_index(reset_index).unwrap();
            counter.reset_backward_by(ticks);

            assert_eq!(counter.values(), values);
            assert_eq!(
                counter.reset_index(),
                expected_reset,
                "reset_index={reset_index}, ticks={ticks}"
            );
        }
    }
}

#[test]
fn maximal_reset_amounts_obey_full_cycle_behavior() {
    for reset_index in 0..10 {
        let mut counter = TallyCounter::new(3).unwrap();
        counter.set_values(vec![2, 5, 9]).unwrap();
        counter.set_reset_index(reset_index).unwrap();

        let mut forward = counter.clone();
        forward.reset_forward_by(u64::MAX);

        let expected_forward = (reset_index + (u64::MAX % 10) as u8) % 10;

        assert_eq!(forward.values(), &[expected_forward; 3]);
        assert_eq!(forward.reset_index(), expected_forward);

        counter.reset_backward_by(u64::MAX);

        assert_eq!(counter.values(), &[2, 5, 9]);
        assert_eq!(
            counter.reset_index(),
            (reset_index + 10 - (u64::MAX % 10) as u8) % 10
        );
    }
}
