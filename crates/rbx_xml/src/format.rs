//! Format sniffing: distinguishing XML place/model files from other inputs.

/// Returns whether `bytes` looks like a Roblox XML place/model file.
///
/// Only sniffs for the `<roblox` root tag after skipping an optional UTF-8 BOM and
/// leading whitespace or an XML declaration; it does not validate the whole document,
/// since that is what [`crate::deserialize`] is for.
pub fn is_xml(bytes: &[u8]) -> bool {
    let mut rest = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);

    loop {
        rest = trim_leading_ascii_whitespace(rest);
        if let Some(after_tag) = rest.strip_prefix(b"<roblox") {
            // The binary format's magic header is `<roblox!`, one byte longer than
            // this tag name; only whitespace or `>` may legally follow an XML tag
            // name, so checking the next byte tells the two formats apart.
            match after_tag.first() {
                Some(&b) => return b.is_ascii_whitespace() || b == b'>',
                None => return true,
            }
        }
        // Skip a leading `<?xml ... ?>` declaration or an `<!-- ... -->` comment,
        // both of which may precede the root element in a well-formed file.
        if let Some(after) = skip_bracketed(rest, b"<?", b"?>") {
            rest = after;
            continue;
        }
        if let Some(after) = skip_bracketed(rest, b"<!--", b"-->") {
            rest = after;
            continue;
        }
        return false;
    }
}

fn trim_leading_ascii_whitespace(bytes: &[u8]) -> &[u8] {
    let end = bytes
        .iter()
        .position(|&b| !b.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    &bytes[end..]
}

fn skip_bracketed<'a>(bytes: &'a [u8], open: &[u8], close: &[u8]) -> Option<&'a [u8]> {
    if !bytes.starts_with(open) {
        return None;
    }
    let search_from = open.len();
    let close_at = find(&bytes[search_from..], close)?;
    Some(&bytes[search_from + close_at + close.len()..])
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_bare_roblox_tag() {
        assert!(is_xml(b"<roblox version=\"4\">"));
    }

    #[test]
    fn detects_after_bom_and_declaration() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(b"<?xml version=\"1.0\"?>\n<roblox version=\"4\">");
        assert!(is_xml(&bytes));
    }

    #[test]
    fn detects_after_leading_comment() {
        assert!(is_xml(b"<!-- generated --><roblox version=\"4\">"));
    }

    #[test]
    fn rejects_binary_magic() {
        assert!(!is_xml(b"<roblox!\x89\xff\x0d\x0a\x1a\x0a"));
    }

    #[test]
    fn rejects_unrelated_xml() {
        assert!(!is_xml(b"<?xml version=\"1.0\"?><svg></svg>"));
    }
}
