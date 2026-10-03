//! What a template may be called. A name is a file stem, so the rules are
//! the strictest of the three desktop filesystems (Windows' reserved
//! characters, no trailing dot) plus the two the layout itself needs:
//! `Default` means the built-in starter's replacement, and two templates of
//! one class can't share a name in any casing, because on a case-insensitive
//! filesystem they would be the same file.

use std::fmt;

use super::{ScriptTemplates, DEFAULT_STEM};

/// Long enough for any real name, short enough to stay readable in a menu.
pub(crate) const MAX_CHARS: usize = 64;

const FORBIDDEN: &[char] = &['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NameError {
    Empty,
    Forbidden,
    Dot,
    TooLong,
    Reserved,
    Taken { class: &'static str, name: String },
}

impl fmt::Display for NameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NameError::Empty => f.write_str("Names can't be empty."),
            NameError::Forbidden => f.write_str("Names can't contain / \\ : * ? \" < > |"),
            NameError::Dot => f.write_str("Names can't start or end with a dot."),
            NameError::TooLong => write!(f, "Names can't be longer than {MAX_CHARS} characters."),
            NameError::Reserved => f.write_str("\u{201c}Default\u{201d} is reserved for the starter."),
            NameError::Taken { class, name } => write!(
                f,
                "A {class} template called {name} already exists. Names can't repeat inside a class."
            ),
        }
    }
}

/// `name` trimmed, if it passes every rule that doesn't depend on what
/// already exists. Control characters count as forbidden too: a newline in
/// a file name is legal on Linux and a trap everywhere else.
pub(crate) fn check(name: &str) -> Result<&str, NameError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(NameError::Empty);
    }
    if name
        .chars()
        .any(|c| FORBIDDEN.contains(&c) || c.is_control())
    {
        return Err(NameError::Forbidden);
    }
    if name.starts_with('.') || name.ends_with('.') {
        return Err(NameError::Dot);
    }
    if name.chars().count() > MAX_CHARS {
        return Err(NameError::TooLong);
    }
    if name.eq_ignore_ascii_case(DEFAULT_STEM) {
        return Err(NameError::Reserved);
    }
    Ok(name)
}

impl ScriptTemplates {
    /// `name` trimmed, if `class` could take a template called that. `keep`
    /// is the template being renamed, which may keep its own name in another
    /// casing.
    pub(crate) fn check_name(
        &self,
        class: &'static str,
        name: &str,
        keep: Option<&str>,
    ) -> Result<String, NameError> {
        let name = check(name)?;
        let same = |other: &str| other.to_lowercase() == name.to_lowercase();
        if keep.is_none_or(|keep| !same(keep)) && self.taken(class, &same) {
            return Err(NameError::Taken {
                class,
                name: name.to_owned(),
            });
        }
        Ok(name.to_owned())
    }

    /// Whether some file in `class` already has a stem `same` accepts —
    /// a refused file included, since writing over it would lose it.
    fn taken(&self, class: &str, same: &dyn Fn(&str) -> bool) -> bool {
        self.extras
            .iter()
            .any(|t| t.class == class && same(&t.name))
            || self
                .skipped
                .iter()
                .any(|s| s.class == class && s.file_name.strip_suffix(".luau").is_some_and(same))
    }

    /// The first free name in `class` among `<base><suffix>`,
    /// `<base><suffix> 2`, `<base><suffix> 3`…, with `base` shortened so the
    /// whole stays within [`MAX_CHARS`]. `None` if `base` can never be a
    /// name (a forbidden character, say).
    pub(crate) fn free_name(
        &self,
        class: &'static str,
        base: &str,
        suffix: &str,
    ) -> Option<String> {
        let base = base.trim();
        (1..1000).find_map(|n| {
            let tail = if n == 1 {
                suffix.to_owned()
            } else {
                format!("{suffix} {n}")
            };
            let room = MAX_CHARS.saturating_sub(tail.chars().count());
            let head: String = base.chars().take(room).collect();
            self.check_name(class, &format!("{}{tail}", head.trim_end()), None)
                .ok()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::{SkipReason, Skipped, Template};
    use super::{check, NameError, ScriptTemplates, MAX_CHARS};

    fn with(names: &[(&'static str, &str)]) -> ScriptTemplates {
        ScriptTemplates {
            extras: names
                .iter()
                .map(|(class, name)| Template {
                    class,
                    name: (*name).to_owned(),
                    source: String::new(),
                })
                .collect(),
            ..ScriptTemplates::default()
        }
    }

    #[test]
    fn names_are_trimmed_and_checked_for_every_rule() {
        assert_eq!(check("  Door  "), Ok("Door"));
        assert_eq!(check("   "), Err(NameError::Empty));
        for bad in [
            "a/b", "a\\b", "a:b", "a*b", "a?b", "a\"b", "a<b", "a>b", "a|b", "a\nb",
        ] {
            assert_eq!(check(bad), Err(NameError::Forbidden), "{bad:?}");
        }
        assert_eq!(check(".hidden"), Err(NameError::Dot));
        assert_eq!(check("trailing."), Err(NameError::Dot));
        assert_eq!(check("v1.2 helper"), Ok("v1.2 helper"));
        assert!(check(&"x".repeat(MAX_CHARS)).is_ok());
        assert_eq!(check(&"x".repeat(MAX_CHARS + 1)), Err(NameError::TooLong));
        for reserved in ["Default", "default", " DEFAULT "] {
            assert_eq!(check(reserved), Err(NameError::Reserved));
        }
    }

    #[test]
    fn names_repeat_across_classes_but_not_inside_one_in_any_casing() {
        let templates = with(&[("Script", "Door Controller")]);
        assert_eq!(
            templates.check_name("Script", "door controller", None),
            Err(NameError::Taken {
                class: "Script",
                name: "door controller".into()
            })
        );
        assert!(templates
            .check_name("LocalScript", "Door Controller", None)
            .is_ok());
        // Renaming a template may change only its casing.
        assert_eq!(
            templates.check_name("Script", "DOOR CONTROLLER", Some("Door Controller")),
            Ok("DOOR CONTROLLER".into())
        );
    }

    #[test]
    fn a_refused_file_still_holds_its_name() {
        let templates = ScriptTemplates {
            skipped: vec![Skipped {
                class: "Script",
                file_name: "Notes.luau".into(),
                reason: SkipReason::NotUtf8,
            }],
            ..ScriptTemplates::default()
        };
        assert!(templates.check_name("Script", "notes", None).is_err());
    }

    #[test]
    fn the_messages_are_the_ones_the_window_shows() {
        assert_eq!(
            NameError::Taken {
                class: "Script",
                name: "Leaderboard".into()
            }
            .to_string(),
            "A Script template called Leaderboard already exists. Names can't repeat inside a class."
        );
        assert_eq!(
            NameError::Reserved.to_string(),
            "\u{201c}Default\u{201d} is reserved for the starter."
        );
    }

    #[test]
    fn free_names_count_up_and_stay_within_the_limit() {
        let templates = with(&[("Script", "Door"), ("Script", "Door copy")]);
        assert_eq!(
            templates.free_name("Script", "Door", " copy"),
            Some("Door copy 2".into())
        );
        assert_eq!(
            templates.free_name("Script", "Door", ""),
            Some("Door 2".into())
        );
        assert_eq!(
            templates.free_name("Script", "Gate", ""),
            Some("Gate".into())
        );
        assert_eq!(
            templates.free_name("Script", "Default", ""),
            Some("Default 2".into())
        );
        let long = templates
            .free_name("Script", &"y".repeat(MAX_CHARS), " copy")
            .unwrap();
        assert_eq!(long.chars().count(), MAX_CHARS);
        assert!(long.ends_with(" copy"));
        assert_eq!(templates.free_name("Script", "a:b", ""), None);
    }
}
