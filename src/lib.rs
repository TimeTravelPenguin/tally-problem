//! A tally counter simulation and a solver that minimizes increments, then reset ticks.
//!
//! The counter, actions, search results, and errors are independent of any user interface.
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
    Action, SearchError, SearchProgress, SearchResult, SearchSession, SearchStatistics, search,
};
