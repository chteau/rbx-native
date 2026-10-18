//! Live co-editing over the presence relay: every editor in the room sends
//! its DOM edits as operations, and applies everyone's, the sender's own
//! included, in the order the relay logged them.
//!
//! - Identity: an instance's `UniqueId` (32 hex, [`ids`]). An instance the
//!   place file gave no `UniqueId` gets a path-derived id when the session
//!   starts (`p…`, the same on every peer that opened the same file); a new
//!   instance without one gets a fresh random `UniqueId`, written to it as a
//!   real property. Every op also carries the instance's path, because the
//!   Studio plugin cannot read `UniqueId` and resolves by path.
//! - Conflicts: last-writer-wins per (instance, property) on a Lamport clock
//!   ([`Clock`]). Script `Source` is text: an op names the hash of the log
//!   text it was written against, an op whose base is stale is rejected by
//!   every peer alike (they all read the same log). Text that lands while
//!   an editor has its own edits on screen is merged into them three ways
//!   (by line, then by character), so the editor's text always holds both,
//!   and a rejected op of its own is simply sent again on the new log text
//!   ([`engine`]).
//! - Transport: a batch is JSON, cut into chunks, each sealed with
//!   [`crate::crypt::ops_aad`] and appended to the relay's op log
//!   (`/v1/rooms/<room>/ops`). A late joiner replays the log from the start;
//!   every op is idempotent, so replaying onto a newer save converges.

pub mod channel;
pub mod chunk;
pub mod diff;
pub mod engine;
pub mod ids;
pub mod merge;
pub mod names;
pub mod op;
pub mod value;

pub use op::{Batch, Clock, Op, Target};
