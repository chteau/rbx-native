//! String extraction for localization: every user-facing string in the
//! place ([`strings`]), and the `LocalizationTable` they are written into
//! ([`table`]), in the JSON Studio's own table keeps in `Contents`.
//!
//! The pseudo-localization preview that goes with it is the renderer's (see
//! `rbx_viewer::PseudoLocale`); this half is what reaches the place.

mod strings;
mod table;

pub(crate) use strings::{collect, Found, Origin};
pub(crate) use table::{known_sources, write};
#[cfg(test)]
use table::{new_entries, parse, serialize, Entry};

#[cfg(test)]
mod tests;
