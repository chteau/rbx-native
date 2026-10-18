//! Script text: the hash an op's base names, and the three-way merge a
//! sender runs when its edit was written against stale text.

use sha2::{Digest, Sha256};

/// First 16 lowercase hex of SHA-256 of `text`'s UTF-8.
pub fn text_hash(text: &str) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))[..16].to_string()
}

/// The result of [`merge3`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Merged {
    pub text: String,
    /// Overlapping changes were kept side by side between `<<<<<<<` /
    /// `=======` / `>>>>>>>` lines (labelled `ours` / `theirs`), so no text
    /// is lost; the caller says so to the user.
    pub conflicted: bool,
}

/// Line-based three-way merge of `ours` and `theirs` against `base`.
pub fn merge3(base: &str, ours: &str, theirs: &str) -> Merged {
    match diffy::merge(base, ours, theirs) {
        Ok(text) => Merged {
            text,
            conflicted: false,
        },
        Err(text) => Merged {
            text,
            conflicted: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_sixteen_hex_of_sha256() {
        assert_eq!(text_hash(""), "e3b0c44298fc1c14");
        assert_eq!(text_hash("abc"), "ba7816bf8f01cfea");
    }

    #[test]
    fn one_sided_change_merges_clean() {
        let m = merge3("a\nb\nc\n", "a\nB\nc\n", "a\nb\nc\n");
        assert_eq!(m.text, "a\nB\nc\n");
        assert!(!m.conflicted);
    }

    #[test]
    fn non_overlapping_changes_both_land() {
        let m = merge3("a\nb\nc\nd\ne\n", "A\nb\nc\nd\ne\n", "a\nb\nc\nd\nE\n");
        assert_eq!(m.text, "A\nb\nc\nd\nE\n");
        assert!(!m.conflicted);
    }

    #[test]
    fn overlapping_changes_conflict_with_markers() {
        let m = merge3("a\nb\nc\n", "a\nours\nc\n", "a\ntheirs\nc\n");
        assert!(m.conflicted);
        assert!(m.text.contains("<<<<<<<"));
        assert!(m.text.contains("======="));
        assert!(m.text.contains(">>>>>>>"));
        assert!(m.text.contains("ours") && m.text.contains("theirs"));
    }

    #[test]
    fn identical_edits_merge_clean() {
        let m = merge3("a\nb\n", "a\nX\n", "a\nX\n");
        assert_eq!(m.text, "a\nX\n");
        assert!(!m.conflicted);
    }
}
