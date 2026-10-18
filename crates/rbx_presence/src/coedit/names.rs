//! Property names on the wire are Roblox's scripting names; a DOM may hold
//! the same property under a file spelling and in another value kind.

use rbx_dom::{Color3Data, Instance, Variant};
use rbx_reflection::ReflectionDatabase;

/// The scripting name of the property stored as `stored` on `class`.
pub fn wire_name(class: &str, stored: &str) -> String {
    ReflectionDatabase::shared()
        .canonical_name(class, stored)
        .to_string()
}

/// The name `instance` stores the wire property `wire` under: the first
/// spelling it already holds, else the preferred one.
pub fn stored_name(instance: &Instance, wire: &str) -> String {
    let names = ReflectionDatabase::shared().stored_names(instance.class(), wire);
    names
        .iter()
        .find(|n| instance.properties().contains_key(**n))
        .or(names.first())
        .map_or_else(|| wire.to_string(), |n| (*n).to_string())
}

fn as_f64(v: &Variant) -> Option<f64> {
    match v {
        Variant::Float32(x) => Some(f64::from(*x)),
        Variant::Float64(x) => Some(*x),
        Variant::Int32(x) => Some(f64::from(*x)),
        Variant::Int64(x) => Some(*x as f64),
        _ => None,
    }
}

fn byte(x: f32) -> u8 {
    (x * 255.0).round().clamp(0.0, 255.0) as u8
}

/// `value` converted to the kind of `existing` when the two differ and a
/// conversion is known; otherwise `value` unchanged.
pub fn coerce(existing: Option<&Variant>, value: Variant) -> Variant {
    let Some(existing) = existing else {
        return value;
    };
    match (existing, &value) {
        (Variant::Color3uint8 { .. }, Variant::Color3(c)) => Variant::Color3uint8 {
            r: byte(c.r),
            g: byte(c.g),
            b: byte(c.b),
        },
        (Variant::Color3(_), Variant::Color3uint8 { r, g, b }) => Variant::Color3(Color3Data {
            r: f32::from(*r) / 255.0,
            g: f32::from(*g) / 255.0,
            b: f32::from(*b) / 255.0,
        }),
        _ => {
            let Some(x) = as_f64(&value) else {
                return value;
            };
            match existing {
                Variant::Float32(_) if !matches!(value, Variant::Float32(_)) => {
                    Variant::Float32(x as f32)
                }
                Variant::Float64(_) if !matches!(value, Variant::Float64(_)) => Variant::Float64(x),
                Variant::Int32(_) if !matches!(value, Variant::Int32(_)) => {
                    Variant::Int32(x.round() as i32)
                }
                Variant::Int64(_) if !matches!(value, Variant::Int64(_)) => {
                    Variant::Int64(x.round() as i64)
                }
                _ => value,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_dom::WeakDom;

    #[test]
    fn size_and_color_names() {
        assert_eq!(wire_name("Part", "size"), "Size");
        assert_eq!(wire_name("Part", "Color3uint8"), "Color");
        assert_eq!(wire_name("Part", "Anchored"), "Anchored");
    }

    #[test]
    fn stored_round_trip() {
        let mut dom = WeakDom::new();
        let r = dom.new_instance("Part", "P", None);
        let first = stored_name(dom.get(r).unwrap(), "Size");
        assert_eq!(wire_name("Part", &first), "Size");
        dom.set_property(r, "size", Variant::Float32(1.0)).unwrap();
        let held = stored_name(dom.get(r).unwrap(), "Size");
        assert_eq!(wire_name("Part", &held), "Size");
        assert_eq!(held, "size");
    }

    #[test]
    fn coerce_cases() {
        let u8c = Variant::Color3uint8 { r: 0, g: 0, b: 0 };
        let c = Variant::Color3(Color3Data {
            r: 1.0,
            g: 0.5,
            b: 0.0,
        });
        assert_eq!(
            coerce(Some(&u8c), c),
            Variant::Color3uint8 {
                r: 255,
                g: 128,
                b: 0
            }
        );
        let back = coerce(
            Some(&Variant::Color3(Color3Data {
                r: 0.0,
                g: 0.0,
                b: 0.0,
            })),
            u8c,
        );
        assert_eq!(
            back,
            Variant::Color3(Color3Data {
                r: 0.0,
                g: 0.0,
                b: 0.0
            })
        );
        assert_eq!(
            coerce(Some(&Variant::Int32(0)), Variant::Float64(3.0)),
            Variant::Int32(3)
        );
        assert_eq!(
            coerce(Some(&Variant::Float32(0.0)), Variant::Int64(2)),
            Variant::Float32(2.0)
        );
        assert_eq!(
            coerce(Some(&Variant::BrickColor(1)), Variant::BrickColor(5)),
            Variant::BrickColor(5)
        );
        assert_eq!(coerce(None, Variant::Bool(true)), Variant::Bool(true));
    }
}
