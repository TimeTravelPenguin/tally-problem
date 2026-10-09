//! A tally counter simulation and a solver that minimizes increments, then reset ticks.
//!
//! The counter, actions, search results, and errors are independent of any user interface.
//! [`TallyCounter`] owns the mechanical rules; [`search()`] provides exact
//! lexicographic optimization, [`SearchSession`] supports background batches,
//! and [`increment_lower_bound`] supplies a guaranteed solution-cost milestone.
//! Counter widths are dynamic and leading zeros are significant. Large action
//! counts use `u64`; difficult searches can still exhaust time or memory.
//!
//! ```
//! use tally_problem::{SearchResult, TallyCounter, search};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut counter = TallyCounter::new(2)?;
//! let target = vec![1, 2];
//!
//! if let SearchResult::Found(actions) = search(&counter, &target)? {
//!     for action in actions {
//!         action.apply(&mut counter);
//!     }
//!
//!     assert_eq!(counter.values(), target.as_slice());
//! }
//!
//! # Ok(())
//! # }

pub mod counter;
pub mod search;

pub use counter::{TallyCounter, TallyCounterError};
pub use search::{
    Action, SearchError, SearchProgress, SearchResult, SearchSession, SearchStatistics,
    increment_lower_bound, search,
};
