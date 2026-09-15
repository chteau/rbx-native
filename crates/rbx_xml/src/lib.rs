//! XML place/model format parsing and writing for Roblox `.rbxlx`/`.rbxmx` files.
//!
//! Entry points: [`deserialize`] parses the file into a small generic XML tree
//! once, then walks it to build an in-memory DOM tree with the same `Variant`
//! shapes `rbx_binary` produces for the binary format; [`serialize`] walks a DOM
//! tree the other way, writing the same shapes back out as XML text.

mod deserializer;
mod error;
mod format;
mod serializer;
mod value;
mod xml_tree;

pub use deserializer::deserialize;
pub use error::XmlError;
pub use format::is_xml;
pub use serializer::serialize;
