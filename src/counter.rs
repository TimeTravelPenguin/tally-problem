//! Mechanical counter rules independent of search or presentation.
//!
//! Displays keep their runtime width and leading zeros in most-significant-first
//! order. An increment carries from the right and wraps at that width. The reset
//! knob has ten positions: a forward tick moves wheels equal to the old index to
//! its successor, while a backward tick changes only the knob position. A full
//! forward revolution gathers all wheels into one uniform display, so grouped
//! reset operations can be applied without iterating every tick.

use thiserror::Error;

/// Invalid input supplied to a tally counter.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TallyCounterError {
    /// A counter must have at least one wheel.
    #[error("Invalid digit count: {0}. Value should be greater than 0.")]
    InvalidDigitCount(usize),
    /// Display elements must be decimal digits, including explicit leading zeros.
    #[error("Invalid digit value: {0}. Value should be in the range 0-9.")]
    InvalidDigitValue(u8),
    /// The reset knob has positions zero through nine.
    #[error("Invalid reset index: {0}. Value should be in the range 0-9.")]
    InvalidResetIndex(u8),
}

/// A decimal tally counter with a runtime digit count and a reset knob position.
///
/// Digits are stored from most significant to least significant. Increments wrap
/// at the counter's width. Forward resets engage digits as the knob passes them;
/// backward resets move only the knob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TallyCounter {
    values: Vec<u8>,
    reset_index: u8,
}

impl TallyCounter {
    /// Creates a zeroed counter with its reset knob at zero.
    ///
    /// Returns an error if `digit_count` is zero.
    pub fn new(digit_count: usize) -> Result<Self, TallyCounterError> {
        if digit_count == 0 {
            return Err(TallyCounterError::InvalidDigitCount(digit_count));
        }

        Ok(Self {
            values: vec![0; digit_count],
            reset_index: 0,
        })
    }

    /// Replaces the digits and resizes the counter, preserving the reset position.
    ///
    /// Empty or nondecimal values return an error without changing the counter.
    pub fn set_values(&mut self, new_values: Vec<u8>) -> Result<(), TallyCounterError> {
        if new_values.is_empty() {
            return Err(TallyCounterError::InvalidDigitCount(new_values.len()));
        }

        if let Some(invalid_digit) = new_values.iter().find(|&&digit| digit > 9) {
            return Err(TallyCounterError::InvalidDigitValue(*invalid_digit));
        }

        self.values = new_values;

        Ok(())
    }

    /// Sets the reset knob position without moving any displayed wheels.
    ///
    /// Invalid positions return an error without changing the counter.
    pub fn set_reset_index(&mut self, new_reset_index: u8) -> Result<(), TallyCounterError> {
        if new_reset_index > 9 {
            return Err(TallyCounterError::InvalidResetIndex(new_reset_index));
        }

        self.reset_index = new_reset_index;

        Ok(())
    }

    /// Displayed digits from most significant to least significant.
    pub fn values(&self) -> &[u8] {
        &self.values
    }

    /// Number of wheels, including those currently displaying leading zeros.
    pub fn digit_count(&self) -> usize {
        self.values.len()
    }

    /// Internal reset knob position, independent of the displayed value.
    pub fn reset_index(&self) -> u8 {
        self.reset_index
    }

    /// Zeroes the displayed digits and reset position, keeping the counter's width.
    pub fn hard_reset(&mut self) {
        self.values.fill(0);
        self.reset_index = 0;
    }

    /// Adds `amount`, wrapping at the counter's width.
    pub fn increment_by(&mut self, mut amount: u64) {
        for digit in self.values.iter_mut().rev() {
            if amount == 0 {
                break;
            }

            let digit_sum = u64::from(*digit) + amount % 10;
            *digit = (digit_sum % 10) as u8;
            amount = amount / 10 + digit_sum / 10;
        }
    }

    /// Press the increment button once, preserving the reset index.
    pub fn increment(&mut self) {
        self.increment_by(1);
    }

    /// Turns the knob forward, moving each digit that it passes.
    ///
    /// Runs in time proportional to the wheel count, regardless of `ticks`.
    /// Ten or more ticks gather every wheel at the final reset position.
    pub fn reset_forward_by(&mut self, ticks: u64) {
        if ticks == 0 {
            return;
        }

        let distance = (ticks % 10) as u8;
        let next = sum_mod_10(self.reset_index, distance);

        if ticks >= 10 {
            self.values.fill(next);
        } else {
            for value in &mut self.values {
                if sub_mod_10(*value, self.reset_index) < distance {
                    *value = next;
                }
            }
        }

        self.reset_index = next;
    }

    /// Advance one knob position and move wheels matching its old position.
    pub fn tick_reset_forward(&mut self) {
        self.reset_forward_by(1);
    }

    /// Turns the knob backward without changing the displayed digits.
    pub fn reset_backward_by(&mut self, ticks: u64) {
        self.reset_index = sub_mod_10(self.reset_index, (ticks % 10) as u8);
    }

    /// Move the knob backwards one position, leaving all wheel values unchanged.
    pub fn tick_reset_backward(&mut self) {
        self.reset_backward_by(1);
    }
}

#[inline]
fn sum_mod_10(value: u8, amount: u8) -> u8 {
    let sum = value + amount;

    if sum >= 10 { sum - 10 } else { sum }
}

#[inline]
fn sub_mod_10(value: u8, amount: u8) -> u8 {
    if value >= amount {
        value - amount
    } else {
        value + 10 - amount
    }
}
