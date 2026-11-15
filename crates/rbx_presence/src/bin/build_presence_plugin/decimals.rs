//! Finds number literals with a decimal point or an exponent in Luau source.
//! Studio parses those with the system locale on some machines (a French
//! Mac fails on `0.2` with "Malformed number"), which kills the whole plugin,
//! so the plugin writes fractions as integer divisions (`1 / 5`) instead.

/// Each offending literal as `(line, literal)`.
pub(super) fn decimal_literals(source: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    scan(source.as_bytes(), 1, &mut found);
    found
}

fn line_of(bytes: &[u8], end: usize, first: usize) -> usize {
    first + bytes[..end].iter().filter(|&&b| b == b'\n').count()
}

/// The end of a `[[ ]]`/`[==[ ]==]` bracket opened at `at`, if one opens there.
fn long_bracket_end(bytes: &[u8], at: usize) -> Option<usize> {
    let level = bytes[at + 1..].iter().take_while(|&&b| b == b'=').count();
    if bytes.get(at + 1 + level) != Some(&b'[') {
        return None;
    }
    let close: Vec<u8> = [b"]".as_slice(), &vec![b'='; level], b"]"].concat();
    let body = at + 2 + level;
    Some(
        bytes[body..]
            .windows(close.len())
            .position(|w| w == close.as_slice())
            .map_or(bytes.len(), |p| body + p + close.len()),
    )
}

/// The end of a quoted string opened at `at` (just past the closing quote).
fn quoted_end(bytes: &[u8], at: usize) -> usize {
    let quote = bytes[at];
    let mut i = at + 1;
    while i < bytes.len() && bytes[i] != quote {
        i += if bytes[i] == b'\\' { 2 } else { 1 };
    }
    (i + 1).min(bytes.len())
}

fn scan(bytes: &[u8], first_line: usize, found: &mut Vec<(usize, String)>) {
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        let next = bytes.get(i + 1).copied();
        if c == b'-' && next == Some(b'-') {
            i = match (bytes.get(i + 2) == Some(&b'['))
                .then(|| long_bracket_end(bytes, i + 2))
                .flatten()
            {
                Some(end) => end,
                None => bytes[i..]
                    .iter()
                    .position(|&b| b == b'\n')
                    .map_or(bytes.len(), |p| i + p),
            };
        } else if c == b'[' && long_bracket_end(bytes, i).is_some() {
            i = long_bracket_end(bytes, i).unwrap_or(bytes.len());
        } else if c == b'"' || c == b'\'' {
            i = quoted_end(bytes, i);
        } else if c == b'`' {
            // Interpolated: the `{ }` parts are code.
            let end = quoted_end(bytes, i);
            let mut j = i + 1;
            while j < end {
                if bytes[j] == b'\\' {
                    j += 2;
                } else if bytes[j] == b'{' {
                    let close = bytes[j..end]
                        .iter()
                        .position(|&b| b == b'}')
                        .map_or(end, |p| j + p);
                    scan(&bytes[j + 1..close], line_of(bytes, j, first_line), found);
                    j = close + 1;
                } else {
                    j += 1;
                }
            }
            i = end;
        } else if c.is_ascii_alphabetic() || c == b'_' {
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
        } else if c == b'.' && next == Some(b'.') {
            i += 2;
        } else if c.is_ascii_digit() || (c == b'.' && next.is_some_and(|n| n.is_ascii_digit())) {
            let start = i;
            let prefixed = c == b'0' && matches!(next, Some(b'x' | b'X' | b'b' | b'B'));
            while i < bytes.len() {
                let b = bytes[i];
                let exponent_sign =
                    !prefixed && matches!(b, b'+' | b'-') && matches!(bytes[i - 1], b'e' | b'E');
                let point = b == b'.' && bytes.get(i + 1) != Some(&b'.');
                if b.is_ascii_alphanumeric() || b == b'_' || point || exponent_sign {
                    i += 1;
                } else {
                    break;
                }
            }
            let literal = &bytes[start..i];
            let has_exponent = !prefixed && literal.iter().any(|b| matches!(b, b'e' | b'E'));
            if literal.contains(&b'.') || has_exponent {
                found.push((
                    line_of(bytes, start, first_line),
                    String::from_utf8_lossy(literal).into_owned(),
                ));
            }
        } else {
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_decimals_and_exponents_only_in_code() {
        let src = "local a = 0.2\nlocal b = .5 + 1e3 + 2E-4\nlocal c = 1 / 5\n\
            local d = 0xFF + 0b1010 + 1_000 + x2.y\nlocal e = 1 .. \"3.5\" .. '0.1' .. [[0.25]]\n\
            -- 0.75 in a comment\n--[==[ 9.5 ]==]\nlocal f = `{1.5} and 2.5`\nlocal g = t[1]..\"x\"";
        let got: Vec<(usize, String)> = decimal_literals(src);
        let want: Vec<(usize, String)> =
            [(1, "0.2"), (2, ".5"), (2, "1e3"), (2, "2E-4"), (8, "1.5")]
                .iter()
                .map(|&(l, s)| (l, s.to_owned()))
                .collect();
        assert_eq!(got, want);
    }

    /// Every script the built plugin carries, generated modules included.
    #[test]
    fn built_plugin_has_no_decimal_literals() {
        let xml = super::super::build(&super::super::plugin_dir().join("src")).expect("build");
        let dom = rbx_xml::deserialize(&xml).expect("deserialize");
        let root = dom.root_refs()[0];
        let mut scripts = vec![root];
        scripts.extend(dom.get(root).expect("root").children());
        let mut bad = Vec::new();
        for script in scripts {
            let instance = dom.get(script).expect("script");
            let Some(rbx_dom::Variant::String(source)) = instance.properties().get("Source") else {
                panic!("{}: no Source", instance.name());
            };
            for (line, literal) in decimal_literals(source) {
                bad.push(format!("{}:{line}: {literal}", instance.name()));
            }
        }
        assert!(bad.is_empty(), "write these as integer divisions: {bad:#?}");
    }
}
