//! Splits expression source into tokens.

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Tok {
    Num(f64),
    Str(String),
    Name(String),
    Sym(&'static str),
}

const SYMBOLS: [&str; 22] = [
    "//", "..", "==", "~=", "<=", ">=", "+", "-", "*", "/", "%", "^", "<", ">", "(", ")", ",", "=",
    ".", ";", "#", "!",
];

pub(super) fn lex(src: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '-' && chars.get(i + 1) == Some(&'-') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c.is_ascii_digit()
            || (c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit))
        {
            let start = i;
            while i < chars.len()
                && (chars[i].is_ascii_alphanumeric() || chars[i] == '.' || chars[i] == '_')
            {
                // An exponent's sign belongs to the number.
                if matches!(chars[i], 'e' | 'E') && matches!(chars.get(i + 1), Some('+' | '-')) {
                    i += 1;
                }
                i += 1;
            }
            let text: String = chars[start..i].iter().filter(|c| **c != '_').collect();
            let n = if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
                i64::from_str_radix(hex, 16).map(|v| v as f64).ok()
            } else {
                text.parse().ok()
            };
            out.push(Tok::Num(
                n.ok_or_else(|| format!("malformed number {text}"))?,
            ));
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push(Tok::Name(chars[start..i].iter().collect()));
        } else if c == '"' || c == '\'' {
            let mut s = String::new();
            i += 1;
            while i < chars.len() && chars[i] != c {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    i += 1;
                    s.push(match chars[i] {
                        'n' => '\n',
                        't' => '\t',
                        other => other,
                    });
                } else {
                    s.push(chars[i]);
                }
                i += 1;
            }
            if i >= chars.len() {
                return Err("unfinished string".into());
            }
            i += 1;
            out.push(Tok::Str(s));
        } else {
            let rest: String = chars[i..chars.len().min(i + 2)].iter().collect();
            let sym = SYMBOLS
                .iter()
                .find(|s| rest.starts_with(**s))
                .ok_or_else(|| format!("unexpected '{c}'"))?;
            i += sym.chars().count();
            out.push(Tok::Sym(sym));
        }
    }
    Ok(out)
}
