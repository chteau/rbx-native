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
    if template.trim().is_empty() {
        return None;
    }
    // The sandbox may already carry the mark (a relink reads its current
    // name back): take it off first, so "Obby (Play)" stays "Obby (Play)".
    let mut place = place.trim();
    if let Some((before, after)) = template.split_once("{place}") {
        let (before, after) = (before.trim_start(), after.trim_end());
        if !(before.is_empty() && after.is_empty()) && !after.contains("{place}") {
            while let Some(inner) = place
                .strip_prefix(before)
                .and_then(|rest| rest.strip_suffix(after))
            {
                place = inner.trim();
            }
        }
    }
    let replaced = template.replace("{place}", place);
    let cut: String = replaced.trim().chars().take(MAX_CHARS).collect();
    Some(cut.trim().to_owned())
}

#[cfg(test)]
mod tests;
