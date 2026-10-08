pub mod search;

pub use search::{Action, search};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TallyCounter<const N: usize> {
    values: [u8; N],
    reset_index: u8,
}

impl<const N: usize> Default for TallyCounter<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> TallyCounter<N> {
    pub fn new() -> Self {
        Self {
            values: [0; N],
            reset_index: 0,
        }
    }

    pub fn set_values(&mut self, new_values: [u8; N]) {
        if new_values.iter().any(|&v| v >= 10) {
            panic!("All values must be in the range 0-9");
        }

        self.values = new_values;
    }

    pub fn set_reset_index(&mut self, new_reset_index: u8) {
        if new_reset_index >= 10 {
            panic!("Reset index must be in the range 0-9");
        }

        self.reset_index = new_reset_index;
    }

    pub fn values(&self) -> &[u8; N] {
        &self.values
    }

    pub fn reset_index(&self) -> u8 {
        self.reset_index
    }

    pub fn hard_reset(&mut self) {
        self.values = [0; N];
        self.reset_index = 0;
    }

    pub fn increment_by(&mut self, mut amount: u64) {
        for digit in self.values.iter_mut().rev() {
            if amount == 0 {
                break;
            }

            let sum = *digit as u64 + amount;
            *digit = (sum % 10) as u8;
            amount = sum / 10;
        }
    }

    pub fn increment(&mut self) {
        self.increment_by(1);
    }

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

    pub fn tick_reset_forward(&mut self) {
        self.reset_forward_by(1);
    }

    pub fn reset_backward_by(&mut self, ticks: u64) {
        self.reset_index = sub_mod_10(self.reset_index, (ticks % 10) as u8);
    }

    pub fn tick_reset_backward(&mut self) {
        self.reset_backward_by(1);
    }
}

#[inline]
pub fn sum_mod_10(a: u8, b: u8) -> u8 {
    let sum = a + b;
    if sum >= 10 { sum - 10 } else { sum }
}

#[inline]
pub fn sub_mod_10(a: u8, b: u8) -> u8 {
    if a >= b { a - b } else { a + 10 - b }
}
