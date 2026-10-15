//! Structured `print` for scripts that run in a Play session.
//!
//! `LogService.MessageOut` hands the probe a `print`ed table as the flat
//! `table: 0x…` string, so the table itself has to be caught where it is
//! printed. Every game script that mentions `print` gets [`PREFIX`] inserted
//! right before its first token, on that same line (line numbers, and so
//! errors and breakpoints, stay as written; `--!` directives stay first):
//!
//! ```lua
//! local print = require(game:GetService("ReplicatedStorage"):WaitForChild("__rbxnative_print")).wrap(print);print({1, 2})
//! ```
//!
//! The probe's `__rbxnative_print` module (`probe/print.luau`) walks the
//! arguments and notes the caller's script and line before calling the real
//! `print`; the probe pairs that record with the `MessageOut` line it causes.

use full_moon::node::Node;
use full_moon::tokenizer::TokenType;

/// Name of the ModuleScript the rewritten scripts require.
pub const PRINT_NAME: &str = "__rbxnative_print";

/// Inserted before a script's first token.
pub const PREFIX: &str = "local print = require(game:GetService(\"ReplicatedStorage\"):WaitForChild(\"__rbxnative_print\")).wrap(print);";

/// `source` with [`PREFIX`] inserted right before its first token.
/// `Ok(None)` when `source` never says `print` or has no token at all,
/// `Err` with the parser's message when it does not parse (the caller
/// publishes it as is).
pub fn rewrite(source: &str) -> Result<Option<String>, String> {
    if !source.contains("print") {
        return Ok(None);
    }
    let ast = full_moon::parse(source).map_err(|errors| {
        errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    let first = ast.nodes().tokens().find(|t| {
        !matches!(
            t.token().token_type(),
            TokenType::Whitespace { .. }
                | TokenType::SingleLineComment { .. }
                | TokenType::MultiLineComment { .. }
                | TokenType::Eof
        )
    });
    let Some(first) = first else {
        return Ok(None);
    };
    let at = first.token().start_position().bytes();
    let mut out = String::with_capacity(source.len() + PREFIX.len());
    out.push_str(&source[..at]);
    out.push_str(PREFIX);
    out.push_str(&source[at..]);
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &str) -> usize {
        s.matches('\n').count()
    }

    #[test]
    fn plain_script_gets_prefix_first() {
        let src = "print(1)\nprint(2)";
        let out = rewrite(src).unwrap().unwrap();
        assert_eq!(out, format!("{PREFIX}{src}"));
        assert_eq!(lines(&out), lines(src));
    }

    #[test]
    fn directive_stays_first() {
        let src = "--!strict\nprint(1)";
        let out = rewrite(src).unwrap().unwrap();
        assert_eq!(out, format!("--!strict\n{PREFIX}print(1)"));
        assert_eq!(lines(&out), lines(src));
    }

    #[test]
    fn block_comment_same_line() {
        let src = "--[[ a\nb ]] print(1)";
        let out = rewrite(src).unwrap().unwrap();
        assert_eq!(out, format!("--[[ a\nb ]] {PREFIX}print(1)"));
        assert_eq!(lines(&out), lines(src));
    }

    #[test]
    fn nothing_to_do() {
        assert_eq!(rewrite("local x = 1").unwrap(), None);
        assert_eq!(rewrite("-- print later\n").unwrap(), None);
    }

    #[test]
    fn parse_error() {
        assert!(rewrite("print(").is_err());
    }
}
