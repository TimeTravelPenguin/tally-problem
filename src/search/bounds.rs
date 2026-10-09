//! Increment bounds derived from wheel equality and mandatory final increments.
//!
//! A reset applies the same digit mapping to every wheel, so it cannot split an
//! equal neighboring pair. The maximum weighted set of disjoint unequal pairs
//! is a potential that resets cannot increase and one increment increases by at
//! most one. Its increase between two displays is therefore a lower bound.
//! A carry replaces a trailing run of nines with zeros: if it creates a boundary
//! before the carry digit, an old boundary just after it has weight one smaller.
//! Substituting that old boundary in any newly optimal matching proves the
//! one-increment limit; a carry-free increment changes only the weight-one edge.
//!
//! A display-changing forward tick also removes its old reset digit entirely.
//! When a target contains all ten digits, every display after the greatest
//! smaller display missing a digit must be reached by increments. Finding that
//! predecessor takes ten linear scans, rather than enumerating the final stretch.
//! These bounds concern increment counts, not diagram work or elapsed time.

use super::SearchError;

/// Maximum weight of disjoint unequal neighboring pairs, in one backward pass.
///
/// Boundary `idx` has weight `digits.len() - idx - 1`. Skipping it uses the next
/// optimum; selecting it adds its weight to the optimum two boundaries away.
/// The two rolling scores avoid allocating a table proportional to the width.
/// Absolute scores use `u128` so a small score difference is representable even
/// when the scores themselves exceed `u64`. Their quadratic bound fits `u128`
/// for both the supported 32-bit and 64-bit address spaces.
pub(super) fn boundary_score(digits: &[u8]) -> u128 {
    let mut next = 0_u128;
    let mut after_next = 0_u128;

    for idx in (0..digits.len().saturating_sub(1)).rev() {
        let score = if digits[idx] != digits[idx + 1] {
            let weight = (digits.len() - idx - 1) as u128;

            next.max(weight + after_next)
        } else {
            next
        };

        after_next = next;
        next = score;
    }

    next
}

/// Greatest same-width predecessor missing a digit, or no forced final stretch.
///
/// Leading zeros count as digit occurrences. For each excluded digit, preserve
/// the longest possible target prefix, lower the first conflicting digit (or
/// backtrack), and fill the remaining positions with the largest allowed digit.
pub(super) fn forced_predecessor(target: &[u8]) -> Result<Option<Vec<u8>>, SearchError> {
    let mask = target.iter().fold(0_u16, |acc, digit| acc | (1 << digit));

    if mask != 0x3ff {
        return Ok(None);
    }

    let mut best = None;

    for excluded in 0..10 {
        let first = target.iter().position(|&digit| digit == excluded).unwrap();
        let replacement = (0..=first).rev().find_map(|idx| {
            (0..target[idx])
                .rev()
                .find(|&digit| digit != excluded)
                .map(|digit| (idx, digit))
        });

        let Some((idx, digit)) = replacement else {
            continue;
        };

        let mut candidate = Vec::new();
        candidate
            .try_reserve_exact(target.len())
            .map_err(|_| SearchError::AllocationFailed)?;
        candidate.extend_from_slice(target);
        candidate[idx] = digit;
        candidate[idx + 1..].fill(if excluded == 9 { 8 } else { 9 });

        if best.as_ref().is_none_or(|previous| candidate > *previous) {
            best = Some(candidate);
        }
    }

    Ok(best)
}

/// Subtract ordered, equally wide decimal displays without encoding their values.
///
/// Only the difference must fit `u64`; the displays themselves can be arbitrarily
/// wide. Zero digits beyond the representable place values do not overflow.
pub(super) fn decimal_difference(upper: &[u8], lower: &[u8]) -> Result<u64, SearchError> {
    debug_assert_eq!(upper.len(), lower.len());
    debug_assert!(upper >= lower);

    let mut borrow = 0_i8;
    let mut place = Some(1_u64);
    let mut difference = 0_u64;

    for (&upper_digit, &lower_digit) in upper.iter().zip(lower).rev() {
        let digit = upper_digit as i8 - lower_digit as i8 - borrow;
        borrow = i8::from(digit < 0);
        let digit = (digit + borrow * 10) as u64;

        if digit != 0 {
            let part = place
                .and_then(|place| place.checked_mul(digit))
                .ok_or(SearchError::CostTooLarge)?;
            difference = difference
                .checked_add(part)
                .ok_or(SearchError::CostTooLarge)?;
        }

        place = place.and_then(|place| place.checked_mul(10));
    }

    Ok(difference)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TallyCounter;

    fn digits(value: &str) -> Vec<u8> {
        value.bytes().map(|digit| digit - b'0').collect()
    }

    #[test]
    fn potential_is_consistent_with_every_four_digit_transition() {
        let mut counter = TallyCounter::new(4).unwrap();

        for value in 0..10_000 {
            counter.set_values(digits(&format!("{value:04}"))).unwrap();
            let before = boundary_score(counter.values());
            let mut incremented = counter.clone();
            incremented.increment();
            assert!(boundary_score(incremented.values()) <= before + 1);

            for reset in 0..10 {
                let mut moved = counter.clone();
                moved.set_reset_index(reset).unwrap();
                moved.tick_reset_forward();
                assert!(boundary_score(moved.values()) <= before);
            }
        }
    }

    #[test]
    fn forced_predecessor_counts_leading_zeros_and_backtracks() {
        assert_eq!(forced_predecessor(&digits("9876")).unwrap(), None);
        assert_eq!(
            forced_predecessor(&digits("012345678999")).unwrap(),
            Some(digits("012345678888"))
        );

        for target in ["0123456789", "1023456789", "9876543210", "1203456789"] {
            let target_digits = digits(target);
            let predecessor = forced_predecessor(&target_digits).unwrap().unwrap();
            let target_value: u64 = target.parse().unwrap();
            let expected = (0..target_value)
                .rev()
                .find(|value| {
                    let candidate = digits(&format!("{value:010}"));
                    (0..10).any(|digit| !candidate.contains(&digit))
                })
                .unwrap();

            assert_eq!(predecessor, digits(&format!("{expected:010}")));
        }
    }

    #[test]
    fn differences_support_wide_values_and_report_only_difference_overflow() {
        assert_eq!(
            decimal_difference(&digits("012345678999"), &digits("012345678888")),
            Ok(111)
        );
        let prefix = "9".repeat(1_000);
        assert_eq!(
            decimal_difference(
                &digits(&format!("{prefix}10")),
                &digits(&format!("{prefix}09"))
            ),
            Ok(1)
        );
        assert_eq!(
            decimal_difference(
                &digits("18446744073709551615"),
                &digits("00000000000000000000")
            ),
            Ok(u64::MAX)
        );
        assert_eq!(
            decimal_difference(
                &digits("18446744073709551616"),
                &digits("00000000000000000000")
            ),
            Err(SearchError::CostTooLarge)
        );
    }
}
