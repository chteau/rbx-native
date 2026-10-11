//! Animation Graph expressions: the Luau subset Roblox's expression nodes and
//! transition conditions accept, evaluated over the graph's parameters.
//!
//! Covered: number, boolean, string and `nil` literals; parameters by name;
//! `+ - * / // % ^`, unary `-`; `..`; comparisons; `and`/`or`/`not` with
//! Luau's truthiness and short-circuiting; `if … then … elseif … else …`
//! expressions; the `math` library's scalar functions and constants; and a
//! body of `local name = expr` lines ending in `return expr`. Vector and
//! CFrame construction (also in Roblox's subset) is not, since nothing in a
//! pose graph's own properties takes one.

use std::collections::BTreeMap;

use crate::graph::model::Value;

/// A value while evaluating; `Nil` is what an unknown name reads as.
#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    Nil,
    Number(f64),
    Bool(bool),
    Text(String),
}

impl Val {
    fn truthy(&self) -> bool {
        !matches!(self, Val::Nil | Val::Bool(false))
    }

    fn number(&self) -> Result<f64, String> {
        match self {
            Val::Number(n) => Ok(*n),
            Val::Text(t) => t.trim().parse().map_err(|_| format!("cannot do arithmetic on \"{t}\"")),
            other => Err(format!("cannot do arithmetic on {}", other.type_name())),
        }
    }

    fn type_name(&self) -> &'static str {
        match self {
            Val::Nil => "nil",
            Val::Number(_) => "number",
            Val::Bool(_) => "boolean",
            Val::Text(_) => "string",
        }
    }

    pub fn into_value(self) -> Value {
        match self {
            Val::Nil => Value::Bool(false),
            Val::Number(n) => Value::Number(n),
            Val::Bool(b) => Value::Bool(b),
            Val::Text(t) => Value::Text(t),
        }
    }
}

impl From<&Value> for Val {
    fn from(v: &Value) -> Val {
        match v {
            Value::Number(n) => Val::Number(*n),
            Value::Bool(b) => Val::Bool(*b),
            Value::Text(t) => Val::Text(t.clone()),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Name(String),
    Sym(&'static str),
}

const SYMBOLS: [&str; 22] = [
    "//", "..", "==", "~=", "<=", ">=", "+", "-", "*", "/", "%", "^", "<", ">", "(", ")", ",",
    "=", ".", ";", "#", "!",
];

fn lex(src: &str) -> Result<Vec<Tok>, String> {
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
        } else if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit)) {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '.' || chars[i] == '_') {
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
            out.push(Tok::Num(n.ok_or_else(|| format!("malformed number {text}"))?));
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

struct Parser<'a> {
    toks: Vec<Tok>,
    at: usize,
    params: &'a dyn Fn(&str) -> Option<Value>,
    locals: BTreeMap<String, Val>,
    /// Above zero while parsing a branch Luau would not evaluate (the right
    /// of a short-circuited `and`/`or`, an `if`'s untaken arm): it is still
    /// parsed, but its runtime errors are not raised.
    dry: u32,
}

fn binding(op: &str) -> Option<(u8, u8)> {
    // (left, right) binding power; right-associative ones bind tighter right.
    Some(match op {
        "or" => (1, 2),
        "and" => (3, 4),
        "<" | ">" | "<=" | ">=" | "~=" | "==" => (5, 6),
        ".." => (8, 7),
        "+" | "-" => (9, 10),
        "*" | "/" | "//" | "%" => (11, 12),
        "^" => (16, 15),
        _ => return None,
    })
}

const UNARY: u8 = 13;

impl Parser<'_> {
    fn soft(&self, result: Result<Val, String>) -> Result<Val, String> {
        match result {
            Err(_) if self.dry > 0 => Ok(Val::Nil),
            other => other,
        }
    }

    fn skipped(&mut self, min: u8) -> Result<Val, String> {
        self.dry += 1;
        let result = self.expr(min);
        self.dry -= 1;
        result
    }

    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.at).cloned();
        self.at += 1;
        t
    }

    fn is(&self, word: &str) -> bool {
        matches!(self.peek(), Some(Tok::Name(n)) if n == word)
            || matches!(self.peek(), Some(Tok::Sym(s)) if *s == word)
    }

    fn expect(&mut self, word: &str) -> Result<(), String> {
        if self.is(word) {
            self.at += 1;
            Ok(())
        } else {
            Err(format!("expected '{word}'"))
        }
    }

    /// A body: `local` lines then a `return`, or a bare expression.
    fn body(&mut self) -> Result<Val, String> {
        loop {
            if self.is("local") {
                self.at += 1;
                let Some(Tok::Name(name)) = self.next() else {
                    return Err("expected a name after 'local'".into());
                };
                self.expect("=")?;
                let value = self.expr(0)?;
                self.locals.insert(name, value);
                if self.is(";") {
                    self.at += 1;
                }
                continue;
            }
            if self.is("return") {
                self.at += 1;
            }
            let value = self.expr(0)?;
            if self.is(";") {
                self.at += 1;
            }
            return match self.peek() {
                None => Ok(value),
                Some(t) => Err(format!("unexpected {t:?} after the expression")),
            };
        }
    }

    fn expr(&mut self, min: u8) -> Result<Val, String> {
        let mut left = self.prefix()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Sym(s)) => *s,
                Some(Tok::Name(n)) if n == "and" || n == "or" => {
                    if n == "and" {
                        "and"
                    } else {
                        "or"
                    }
                }
                _ => break,
            };
            let Some((l, r)) = binding(op) else { break };
            if l < min {
                break;
            }
            self.at += 1;
            left = match op {
                "and" if left.truthy() => self.expr(r)?,
                "and" => {
                    self.skipped(r)?;
                    left
                }
                "or" if left.truthy() => {
                    self.skipped(r)?;
                    left
                }
                "or" => self.expr(r)?,
                _ => {
                    let right = self.expr(r)?;
                    self.soft(binary(op, left, right))?
                }
            };
        }
        Ok(left)
    }

    fn prefix(&mut self) -> Result<Val, String> {
        match self.next() {
            Some(Tok::Num(n)) => Ok(Val::Number(n)),
            Some(Tok::Str(s)) => Ok(Val::Text(s)),
            Some(Tok::Sym("-")) => {
                let operand = self.expr(UNARY)?;
                self.soft(operand.number().map(|n| Val::Number(-n)))
            }
            Some(Tok::Sym("(")) => {
                let v = self.expr(0)?;
                self.expect(")")?;
                Ok(v)
            }
            Some(Tok::Name(n)) => match n.as_str() {
                "true" => Ok(Val::Bool(true)),
                "false" => Ok(Val::Bool(false)),
                "nil" => Ok(Val::Nil),
                "not" => Ok(Val::Bool(!self.expr(UNARY)?.truthy())),
                "if" => self.if_expr(),
                "math" => self.math(),
                _ => Ok(self
                    .locals
                    .get(&n)
                    .cloned()
                    .or_else(|| (self.params)(&n).map(|v| Val::from(&v)))
                    .unwrap_or(Val::Nil)),
            },
            other => Err(format!("unexpected {other:?}")),
        }
    }

    fn if_expr(&mut self) -> Result<Val, String> {
        let cond = self.expr(0)?.truthy();
        self.expect("then")?;
        let yes = if cond { self.expr(0)? } else { self.skipped(0)? };
        let no = if self.is("elseif") {
            self.at += 1;
            if cond {
                self.dry += 1;
                let rest = self.if_expr();
                self.dry -= 1;
                rest?
            } else {
                self.if_expr()?
            }
        } else {
            self.expect("else")?;
            if cond { self.skipped(0)? } else { self.expr(0)? }
        };
        Ok(if cond { yes } else { no })
    }

    fn math(&mut self) -> Result<Val, String> {
        self.expect(".")?;
        let Some(Tok::Name(f)) = self.next() else {
            return Err("expected a math function".into());
        };
        match f.as_str() {
            "pi" => return Ok(Val::Number(std::f64::consts::PI)),
            "huge" => return Ok(Val::Number(f64::INFINITY)),
            _ => {}
        }
        self.expect("(")?;
        let mut args = Vec::new();
        if !self.is(")") {
            loop {
                let arg = self.expr(0)?;
                args.push(match self.soft(arg.number().map(Val::Number))? {
                    Val::Number(n) => n,
                    _ => 0.,
                });
                if !self.is(",") {
                    break;
                }
                self.at += 1;
            }
        }
        self.expect(")")?;
        self.soft(math(&f, &args).map(Val::Number))
    }
}

fn math(f: &str, a: &[f64]) -> Result<f64, String> {
    let arg = |i: usize| a.get(i).copied().ok_or_else(|| format!("math.{f} needs more arguments"));
    Ok(match f {
        "abs" => arg(0)?.abs(),
        "acos" => arg(0)?.acos(),
        "asin" => arg(0)?.asin(),
        "atan" => arg(0)?.atan(),
        "atan2" => arg(0)?.atan2(arg(1)?),
        "ceil" => arg(0)?.ceil(),
        "floor" => arg(0)?.floor(),
        "cos" => arg(0)?.cos(),
        "sin" => arg(0)?.sin(),
        "tan" => arg(0)?.tan(),
        "cosh" => arg(0)?.cosh(),
        "sinh" => arg(0)?.sinh(),
        "tanh" => arg(0)?.tanh(),
        "deg" => arg(0)?.to_degrees(),
        "rad" => arg(0)?.to_radians(),
        "exp" => arg(0)?.exp(),
        "log" => match a.get(1) {
            Some(base) => arg(0)?.log(*base),
            None => arg(0)?.ln(),
        },
        "log10" => arg(0)?.log10(),
        "sqrt" => arg(0)?.sqrt(),
        "pow" => arg(0)?.powf(arg(1)?),
        "fmod" => arg(0)? % arg(1)?,
        "modf" => arg(0)?.trunc(),
        "round" => arg(0)?.round(),
        "sign" => {
            let x = arg(0)?;
            if x > 0. { 1. } else if x < 0. { -1. } else { 0. }
        }
        "clamp" => arg(0)?.clamp(arg(1)?, arg(2)?.max(arg(1)?)),
        "min" => a.iter().copied().reduce(f64::min).ok_or("math.min needs arguments")?,
        "max" => a.iter().copied().reduce(f64::max).ok_or("math.max needs arguments")?,
        "lerp" => arg(0)? + (arg(1)? - arg(0)?) * arg(2)?,
        "map" => {
            let (x, i0, i1, o0, o1) = (arg(0)?, arg(1)?, arg(2)?, arg(3)?, arg(4)?);
            o0 + (x - i0) * (o1 - o0) / (i1 - i0)
        }
        other => return Err(format!("math.{other} is not available in expressions")),
    })
}

fn binary(op: &str, l: Val, r: Val) -> Result<Val, String> {
    Ok(match op {
        "==" => Val::Bool(l == r),
        "~=" => Val::Bool(l != r),
        ".." => {
            let text = |v: &Val| match v {
                Val::Text(t) => Ok(t.clone()),
                Val::Number(n) => Ok(format!("{n}")),
                other => Err(format!("cannot concatenate {}", other.type_name())),
            };
            Val::Text(text(&l)? + &text(&r)?)
        }
        "<" | ">" | "<=" | ">=" => {
            let ordering = match (&l, &r) {
                (Val::Text(a), Val::Text(b)) => a.partial_cmp(b),
                _ => l.number()?.partial_cmp(&r.number()?),
            };
            let Some(o) = ordering else {
                return Ok(Val::Bool(false));
            };
            Val::Bool(match op {
                "<" => o.is_lt(),
                ">" => o.is_gt(),
                "<=" => o.is_le(),
                _ => o.is_ge(),
            })
        }
        _ => {
            let (a, b) = (l.number()?, r.number()?);
            Val::Number(match op {
                "+" => a + b,
                "-" => a - b,
                "*" => a * b,
                "/" => a / b,
                "//" => (a / b).floor(),
                "%" => a - (a / b).floor() * b,
                "^" => a.powf(b),
                _ => return Err(format!("unknown operator {op}")),
            })
        }
    })
}

/// Evaluates `source` with `params` supplying each parameter's value.
pub fn eval(source: &str, params: &dyn Fn(&str) -> Option<Value>) -> Result<Val, String> {
    let toks = lex(source)?;
    if toks.is_empty() {
        return Ok(Val::Nil);
    }
    Parser {
        toks,
        at: 0,
        params,
        locals: BTreeMap::new(),
        dry: 0,
    }
    .body()
}

#[cfg(test)]
mod tests;
