//! Luau string patterns, matched by Luau's own `string.find` rather than a
//! reimplementation, so a pattern means here exactly what it means in a
//! script (`%d+`, `^local`, `[%w_]+`, and a malformed one fails the same way).

use mlua::{Function, Lua, Table, Value};

/// One pattern, ready to test text against.
pub struct Pattern {
    // Kept so `find` stays valid; a Luau state of its own, so a host's
    // runtime is never touched.
    _lua: Lua,
    find: Function,
    pattern: String,
}

impl Pattern {
    /// `Err` is Luau's message for a malformed pattern, such as
    /// "malformed pattern (ends with '%')".
    pub fn new(pattern: &str) -> Result<Pattern, String> {
        let lua = crate::luau::new_vm();
        let find = lua
            .globals()
            .get::<Table>("string")
            .and_then(|string| string.get::<Function>("find"))
            .map_err(|err| err.to_string())?;
        let compiled = Pattern {
            _lua: lua,
            find,
            pattern: pattern.to_owned(),
        };
        // Luau only checks a pattern while matching, so it is tried once on
        // nothing to report up front one malformed from its start. One
        // malformed further on fails, as in a script, only on text that
        // matches far enough to reach the fault.
        compiled.is_in("")?;
        Ok(compiled)
    }

    /// Whether `string.find(text, pattern)` finds anything.
    pub fn is_in(&self, text: &str) -> Result<bool, String> {
        let start: Value = self
            .find
            .call((text, self.pattern.as_str()))
            .map_err(|err| err.to_string())?;
        Ok(!start.is_nil())
    }
}

#[cfg(test)]
mod tests {
    use super::Pattern;

    fn found(pattern: &str, text: &str) -> bool {
        Pattern::new(pattern).unwrap().is_in(text).unwrap()
    }

    #[test]
    fn patterns_match_as_luau_matches_them() {
        assert!(found("%d+", "rounds += 1"));
        assert!(!found("%d+", "rounds += one"));
        assert!(found("^local", "local x = 1"));
        assert!(!found("^local", "  local x = 1"));
        // `.` is any character, as in a script; `%.` is the dot itself.
        assert!(found("a.c", "abc"));
        assert!(!found("a%.c", "abc"));
        assert!(found("print", "print(\"hi\")"));
    }

    #[test]
    fn a_malformed_pattern_is_refused_with_luaus_message() {
        let err = Pattern::new("%").err().unwrap();
        assert!(err.contains("malformed pattern"), "{err}");
        assert!(Pattern::new("[a-").is_err());
        // Luau only finds the fault when matching reaches it, so a pattern
        // that fails before then is refused only by text that gets there,
        // exactly as in a script.
        let late = Pattern::new("50%").unwrap();
        assert!(!late.is_in("abc").unwrap());
        assert!(late.is_in("50").is_err());
    }
}
