//! The name Play gives the sandbox place while it runs a test
//! (Settings › Files › Play › Name for test copies): what the Roblox
//! client's title, the server list and the place's own page show.

/// Studio-like default: the place's name, marked as a test.
pub(crate) const DEFAULT: &str = "{place} (Play)";

/// Roblox refuses longer place names.
pub(crate) const MAX_CHARS: usize = 50;

/// `template` with every `{place}` replaced by `place`, trimmed, and cut to
/// [`MAX_CHARS`] characters (never inside a character). `None` when the
/// template is blank: Play then leaves the sandbox's name alone.
pub(crate) fn render(template: &str, place: &str) -> Option<String> {
    todo!()
}

#[cfg(test)]
mod tests;
