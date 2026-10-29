//! Bridge Contract bridge.
//!
//! The session cell is a point of the product machine
//! `Attachment x Human x Lifecycle`, derived from a client set. Every
//! transition appends its events to the durable [`Storage`] as one atomic
//! batch and only then changes the in-memory cell (invariants I7, I12);
//! restart recovery replays the log.
//!
//! Written from the paper text (Sections 4 and 5) and the vector schema in
//! `../vectors/README.md`, not by porting `conformance_suite.py`; the author
//! is the same, so independence is weak (see README.md).

mod bridge;
mod types;

pub use bridge::{AuthorityOutcome, Bridge, Source, ToolOutcome};
pub use types::*;
