//! Parsers for each chunk type in the Roblox binary format.
//!
//! Each module owns the layout of its own chunk payload (INST, PRNT, PROP, SSTR)
//! and knows nothing about the others; `deserializer` is what stitches them back into a tree.

pub(crate) mod inst;
pub(crate) mod prnt;
pub(crate) mod prop;
pub(crate) mod sstr;
