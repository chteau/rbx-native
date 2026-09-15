//! Binary format parsing for Roblox .rbxm and .rbxl files.
//!
//! Entry point: [`deserialize`]. Handles decompression, chunk parsing, and
//! construction of an in-memory DOM tree with property decoding.

mod chunk;
mod chunks;
mod codec;
mod deserializer;
mod error;
mod header;
mod serialize;

pub use chunk::{read_chunks, Chunk, ChunkReader};
pub use deserializer::deserialize;
pub use error::BinaryError;
pub use header::{parse_header, FileHeader};
pub use serialize::{serialize, SerializeError};
