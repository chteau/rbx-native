//! Operators and the `math` library.

use super::{types, Val};

pub(super) fn math(f: &str, a: &[f64]) -> Result<f64, String> {
    let arg = |i: usize| {
        a.get(i)
            .copied()
            .ok_or_else(|| format!("math.{f} needs more arguments"))
    };
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
            if x > 0. {
                1.
            } else if x < 0. {
                -1.
            } else {
                0.
            }
        }
        "clamp" => arg(0)?.clamp(arg(1)?, arg(2)?.max(arg(1)?)),
        "min" => a
            .iter()
            .copied()
            .reduce(f64::min)
            .ok_or("math.min needs arguments")?,
        "max" => a
            .iter()
            .copied()
            .reduce(f64::max)
            .ok_or("math.max needs arguments")?,
        "lerp" => arg(0)? + (arg(1)? - arg(0)?) * arg(2)?,
        "map" => {
            let (x, i0, i1, o0, o1) = (arg(0)?, arg(1)?, arg(2)?, arg(3)?, arg(4)?);
            o0 + (x - i0) * (o1 - o0) / (i1 - i0)
        }
        other => return Err(format!("math.{other} is not available in expressions")),
    })
}

pub(super) fn binary(op: &str, l: Val, r: Val) -> Result<Val, String> {
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
            if let Some(result) = types::arith(op, &l, &r) {
                return result;
            }
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
