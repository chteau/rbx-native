//! Error types for the XML format parser.

use thiserror::Error;

// Third-party parser error types aren't part of this crate's public API
// (façade rule, same as rbx_binary): their messages are captured as strings.
/// Errors that can occur while parsing a Roblox XML place/model file.
///
/// Individual malformed properties, unknown tags, or dangling referents degrade
/// gracefully (see the `value` module and `deserializer`) rather than erroring here;
/// this type only covers failures that make the whole document unreadable.
#[derive(Debug, Error)]
pub enum XmlError {
    #[error("malformed XML: {0}")]
    Parse(String),

    #[error("no <roblox> root element found")]
    MissingRoot,

    /// No XML encoder is registered for this `Variant` kind (see `serializer::value`).
    #[error("no XML encoder for the `{0}` property kind")]
    Unsupported(&'static str),
}
