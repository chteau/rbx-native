//! Format Document: StyLua, built in rather than run as a tool the user has
//! to install (<https://devforum.roblox.com/t/build-stylua-into-roblox-studio/3645518>).
//!
//! The script editor and the Command Bar both call [`format`]. StyLua parses
//! with the same `full_moon` the script graph already does, so it brings no
//! second Luau parser into the tree. Its own defaults are kept (tabs, a 120
//! column guide, double quotes): that is the style a StyLua user gets from the
//! command line with no `stylua.toml`, so code formatted here and code
//! formatted by a Rojo project's own StyLua agree.

use stylua_lib::{Config, Error, LuaVersion, OutputVerification};

/// `source` formatted with StyLua's defaults and Luau syntax, or the reason it
/// can't be, phrased for a one-line notice: code that doesn't parse is left
/// alone, StyLua never guesses.
pub(crate) fn format(source: &str) -> Result<String, String> {
    let config = Config {
        syntax: LuaVersion::Luau,
        ..Config::default()
    };
    stylua_lib::format_code(source, config, None, OutputVerification::None).map_err(|error| {
        match error {
            Error::ParseError(errors) => match errors.first() {
                Some(first) => format!(
                    "Can't format, line {}: {}",
                    first.range().0.line(),
                    without_span(&first.error_message())
                ),
                None => "Can't format: the script doesn't parse".to_owned(),
            },
            other => format!("Can't format: {other}"),
        }
    })
}

/// Some of full_moon's messages end with their own `(1:41 to 1:42)`, others
/// say nothing of where; the line is given in front of every one instead.
fn without_span(message: &str) -> &str {
    match message.rsplit_once(" (") {
        Some((head, span)) if span.ends_with(')') && span.contains(" to ") => head,
        _ => message,
    }
}

/// [`format`] for the Command Bar: StyLua ends a file with a newline, which
/// in a one-line field would only add an empty second row.
pub(crate) fn format_command(source: &str) -> Result<String, String> {
    format(source).map(|formatted| formatted.trim_end_matches(['\n', '\r']).to_owned())
}

#[cfg(test)]
mod tests {
    use super::{format, format_command};

    #[test]
    fn formats_luau_with_stylua_defaults() {
        let out = format("local x:number=1 if x then print( 'hi' ) end").unwrap();
        assert_eq!(
            out,
            "local x: number = 1\nif x then\n\tprint(\"hi\")\nend\n"
        );
    }

    #[test]
    fn keeps_luau_only_syntax() {
        let out = format("type P = {name: string}\nn += 1\nlocal s = `a{n}`\n").unwrap();
        assert!(out.contains("type P = { name: string }"), "{out}");
        assert!(out.contains("n += 1"), "{out}");
        assert!(out.contains("`a{n}`"), "{out}");
    }

    #[test]
    fn a_parse_error_names_its_line() {
        let err = format("local a = 1\nlocal = 2\n").unwrap_err();
        assert!(err.starts_with("Can't format, line 2: "), "{err}");
        let err = format("local a = 1\\\n").unwrap_err();
        assert!(err.starts_with("Can't format, line 1: "), "{err}");
        assert!(!err.contains(" to "), "{err}");
    }

    #[test]
    fn command_bar_output_has_no_trailing_newline() {
        assert_eq!(format_command("print( 1 )").unwrap(), "print(1)");
    }
}
