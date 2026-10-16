//! The editor's [`Host`]: the methods of the ABI over a `WeakDom` and a
//! selection.

use rbx_dom::{CFrameData, Color3Data, Ref, Variant, Vector2Data, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;
use serde_json::{json, Value};

use crate::Host;

/// Borrows the place and the selection for one call into a plugin.
/// `set_property` goes through `WeakDom::set_property` (so the change log
/// sees it); `new` applies no defaults beyond the class and name.
pub struct DomHost<'a> {
    pub dom: &'a mut WeakDom,
    pub database: &'a ReflectionDatabase,
    pub selection: &'a mut Vec<Ref>,
    /// Set when `selection.set` ran.
    pub selection_changed: bool,
}

impl Host for DomHost<'_> {
    fn call(&mut self, method: &str, params: &Value) -> Result<Value, String> {
        match method {
            "selection.get" => Ok(refs_json(self.selection)),
            "selection.set" => {
                let list = params
                    .get("refs")
                    .and_then(Value::as_array)
                    .ok_or("selection.set: `refs` must be an array")?;
                let mut refs = Vec::with_capacity(list.len());
                for item in list {
                    refs.push(self.existing(item)?);
                }
                *self.selection = refs;
                self.selection_changed = true;
                Ok(Value::Null)
            }
            "instance.roots" => Ok(refs_json(self.dom.root_refs())),
            "instance.get" => {
                let referent = self.existing(field(params, "ref")?)?;
                let instance = self.dom.get(referent).ok_or("unknown instance")?;
                Ok(json!({
                    "ref": referent.value(),
                    "name": instance.name(),
                    "class": instance.class(),
                    "parent": self.dom.parent(referent).map(|p| p.value()),
                    "children": refs_json(instance.children()),
                }))
            }
            "instance.find" => {
                let path = str_field(params, "path")?;
                Ok(self.find(path).map_or(Value::Null, |r| json!(r.value())))
            }
            "instance.property" => {
                let referent = self.existing(field(params, "ref")?)?;
                let name = str_field(params, "name")?;
                let instance = self.dom.get(referent).ok_or("unknown instance")?;
                match name {
                    "Name" => Ok(json!(instance.name())),
                    "ClassName" => Ok(json!(instance.class())),
                    _ => instance
                        .properties()
                        .get(name)
                        .map(variant_to_json)
                        .ok_or_else(|| format!("{} has no property `{name}`", instance.class())),
                }
            }
            "instance.set_property" => {
                let referent = self.existing(field(params, "ref")?)?;
                let name = str_field(params, "name")?;
                let value = field(params, "value")?;
                if name == "Name" {
                    let text = value.as_str().ok_or("Name must be a string")?;
                    self.dom
                        .set_name(referent, text)
                        .map_err(|e| e.to_string())?;
                    return Ok(Value::Null);
                }
                let current = self
                    .dom
                    .get(referent)
                    .and_then(|i| i.properties().get(name));
                let variant = json_to_variant(value, current)?;
                self.dom
                    .set_property(referent, name, variant)
                    .map_err(|e| e.to_string())?;
                Ok(Value::Null)
            }
            "instance.new" => {
                let class = str_field(params, "class")?;
                if self.database.class(class).is_none() {
                    return Err(format!("unknown class `{class}`"));
                }
                let parent = match params.get("parent") {
                    None | Some(Value::Null) => None,
                    Some(value) => Some(self.existing(value)?),
                };
                let name = params.get("name").and_then(Value::as_str).unwrap_or(class);
                let referent = self.dom.new_instance(class, name, parent);
                Ok(json!(referent.value()))
            }
            "instance.set_parent" => {
                let referent = self.existing(field(params, "ref")?)?;
                let parent = match params.get("parent") {
                    None | Some(Value::Null) => None,
                    Some(value) => Some(self.existing(value)?),
                };
                let mut cursor = parent;
                while let Some(ancestor) = cursor {
                    if ancestor == referent {
                        return Err("cannot parent an instance under itself".to_string());
                    }
                    cursor = self.dom.parent(ancestor);
                }
                self.dom.set_parent(referent, parent);
                Ok(Value::Null)
            }
            "instance.destroy" => {
                let referent = self.existing(field(params, "ref")?)?;
                self.dom.remove(referent);
                self.selection.retain(|r| self.dom.get(*r).is_some());
                Ok(Value::Null)
            }
            other => Err(format!("unknown method `{other}`")),
        }
    }
}

impl DomHost<'_> {
    fn existing(&self, value: &Value) -> Result<Ref, String> {
        let number = value
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or("a ref must be a number")?;
        let referent = Ref::new(number);
        if self.dom.get(referent).is_some() {
            Ok(referent)
        } else {
            Err(format!("no instance with ref {number}"))
        }
    }

    fn find(&self, path: &str) -> Option<Ref> {
        let mut parts = path.split('.');
        let first = parts.next()?;
        let mut current = self
            .dom
            .root_refs()
            .iter()
            .copied()
            .find(|r| self.dom.get(*r).is_some_and(|i| i.name() == first))?;
        for part in parts {
            let instance = self.dom.get(current)?;
            current = instance
                .children()
                .iter()
                .copied()
                .find(|c| self.dom.get(*c).is_some_and(|i| i.name() == part))?;
        }
        Some(current)
    }
}

fn refs_json(refs: &[Ref]) -> Value {
    Value::Array(refs.iter().map(|r| json!(r.value())).collect())
}

fn field<'v>(params: &'v Value, key: &str) -> Result<&'v Value, String> {
    params.get(key).ok_or_else(|| format!("missing `{key}`"))
}

fn str_field<'v>(params: &'v Value, key: &str) -> Result<&'v str, String> {
    field(params, key)?
        .as_str()
        .ok_or_else(|| format!("`{key}` must be a string"))
}

fn num(value: &Value, key: &str) -> Option<f32> {
    value.get(key)?.as_f64().map(|n| n as f32)
}

fn variant_to_json(variant: &Variant) -> Value {
    match variant {
        Variant::Bool(b) => json!(b),
        Variant::String(s) => json!(s),
        Variant::Int32(n) => json!(n),
        Variant::Int64(n) => json!(n),
        Variant::Float32(n) => json!(n),
        Variant::Float64(n) => json!(n),
        Variant::Vector3(v) => json!({"x": v.x, "y": v.y, "z": v.z}),
        Variant::Vector2(v) => json!({"x": v.x, "y": v.y}),
        Variant::Color3(c) => json!({"r": c.r, "g": c.g, "b": c.b}),
        Variant::CFrame(c) => json!({
            "position": [c.position.x, c.position.y, c.position.z],
            "rotation": c.rotation,
        }),
        Variant::Enum(n) => json!({"enum": n}),
        Variant::Ref(r) => json!({"ref": r.value()}),
        other => {
            let debug = format!("{other:?}");
            let kind = debug.split(['(', ' ', '{']).next().unwrap_or("Unknown");
            json!({"unsupported": kind})
        }
    }
}

/// A bare number keeps the numeric type the property already has, since
/// JSON cannot tell an `Int32` from a `Float32`.
fn json_to_variant(value: &Value, current: Option<&Variant>) -> Result<Variant, String> {
    match value {
        Value::Bool(b) => Ok(Variant::Bool(*b)),
        Value::String(s) => Ok(Variant::String(s.clone())),
        Value::Number(n) => {
            let f = n.as_f64().ok_or("number out of range")?;
            Ok(match current {
                Some(Variant::Int32(_)) => Variant::Int32(f as i32),
                Some(Variant::Int64(_)) => Variant::Int64(f as i64),
                Some(Variant::Float64(_)) => Variant::Float64(f),
                Some(Variant::Enum(_)) => Variant::Enum(f as u32),
                _ => Variant::Float32(f as f32),
            })
        }
        Value::Object(map) => {
            if let Some(n) = map.get("enum").and_then(Value::as_u64) {
                return Ok(Variant::Enum(n as u32));
            }
            if let Some(n) = map.get("ref").and_then(Value::as_u64) {
                return Ok(Variant::Ref(Ref::new(n as u32)));
            }
            if map.contains_key("unsupported") {
                return Err("unsupported values cannot be written".to_string());
            }
            if let (Some(position), Some(rotation)) = (
                map.get("position").and_then(Value::as_array),
                map.get("rotation").and_then(Value::as_array),
            ) {
                let p: Vec<f32> = position
                    .iter()
                    .filter_map(|v| v.as_f64())
                    .map(|v| v as f32)
                    .collect();
                let r: Vec<f32> = rotation
                    .iter()
                    .filter_map(|v| v.as_f64())
                    .map(|v| v as f32)
                    .collect();
                if p.len() != 3 || r.len() != 9 {
                    return Err("CFrame needs position[3] and rotation[9]".to_string());
                }
                let mut rotation = [0.0; 9];
                rotation.copy_from_slice(&r);
                return Ok(Variant::CFrame(CFrameData {
                    position: Vector3Data {
                        x: p[0],
                        y: p[1],
                        z: p[2],
                    },
                    rotation,
                }));
            }
            if let (Some(x), Some(y), Some(z)) = (num(value, "x"), num(value, "y"), num(value, "z"))
            {
                return Ok(Variant::Vector3(Vector3Data { x, y, z }));
            }
            if let (Some(x), Some(y)) = (num(value, "x"), num(value, "y")) {
                return Ok(Variant::Vector2(Vector2Data { x, y }));
            }
            if let (Some(r), Some(g), Some(b)) = (num(value, "r"), num(value, "g"), num(value, "b"))
            {
                return Ok(Variant::Color3(Color3Data { r, g, b }));
            }
            Err("unrecognised value object".to_string())
        }
        _ => Err("unsupported value".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> ReflectionDatabase {
        ReflectionDatabase::embedded()
    }

    fn run(
        dom: &mut WeakDom,
        selection: &mut Vec<Ref>,
        method: &str,
        params: Value,
    ) -> Result<Value, String> {
        let database = database();
        let mut host = DomHost {
            dom,
            database: &database,
            selection,
            selection_changed: false,
        };
        host.call(method, &params)
    }

    #[test]
    fn get_find_and_properties() {
        let mut dom = WeakDom::new();
        let ws = dom.new_instance("Workspace", "Workspace", None);
        let part = dom.new_instance("Part", "Part", Some(ws));
        let mut sel = Vec::new();
        let found = run(
            &mut dom,
            &mut sel,
            "instance.find",
            json!({"path": "Workspace.Part"}),
        )
        .unwrap();
        assert_eq!(found, json!(part.value()));
        let missing = run(
            &mut dom,
            &mut sel,
            "instance.find",
            json!({"path": "Workspace.Nope"}),
        )
        .unwrap();
        assert_eq!(missing, Value::Null);
        let info = run(
            &mut dom,
            &mut sel,
            "instance.get",
            json!({"ref": part.value()}),
        )
        .unwrap();
        assert_eq!(info["class"], "Part");
        assert_eq!(info["parent"], json!(ws.value()));
        let roots = run(&mut dom, &mut sel, "instance.roots", json!({})).unwrap();
        assert_eq!(roots, json!([ws.value()]));
        assert!(run(&mut dom, &mut sel, "bogus", json!({})).is_err());
    }

    #[test]
    fn set_property_round_trips() {
        let mut dom = WeakDom::new();
        let part = dom.new_instance("Part", "Part", None);
        let mut sel = Vec::new();
        let cases = [
            json!({"x": 1.0, "y": 2.0, "z": 3.0}),
            json!({"r": 0.5, "g": 0.25, "b": 1.0}),
            json!(true),
            json!(4.5),
            json!("hello"),
        ];
        for (i, value) in cases.iter().enumerate() {
            let name = format!("P{i}");
            run(
                &mut dom,
                &mut sel,
                "instance.set_property",
                json!({"ref": part.value(), "name": name, "value": value}),
            )
            .unwrap();
            let back = run(
                &mut dom,
                &mut sel,
                "instance.property",
                json!({"ref": part.value(), "name": name}),
            )
            .unwrap();
            assert_eq!(&back, value);
        }
    }

    #[test]
    fn unsupported_reads_as_unsupported() {
        let mut dom = WeakDom::new();
        let part = dom.new_instance("Part", "Part", None);
        dom.set_property(
            part,
            "Weird",
            Variant::UDim2(rbx_dom::UDim2 {
                x: rbx_dom::UDim {
                    scale: 0.0,
                    offset: 0,
                },
                y: rbx_dom::UDim {
                    scale: 0.0,
                    offset: 0,
                },
            }),
        )
        .unwrap();
        let mut sel = Vec::new();
        let value = run(
            &mut dom,
            &mut sel,
            "instance.property",
            json!({"ref": part.value(), "name": "Weird"}),
        )
        .unwrap();
        assert_eq!(value, json!({"unsupported": "UDim2"}));
    }

    #[test]
    fn new_parent_destroy_and_selection() {
        let mut dom = WeakDom::new();
        let folder = dom.new_instance("Folder", "Folder", None);
        let mut sel = Vec::new();
        let made = run(
            &mut dom,
            &mut sel,
            "instance.new",
            json!({"class": "Part", "name": "P"}),
        )
        .unwrap();
        let part = made.as_u64().unwrap();
        run(
            &mut dom,
            &mut sel,
            "instance.set_parent",
            json!({"ref": part, "parent": folder.value()}),
        )
        .unwrap();
        assert_eq!(dom.parent(Ref::new(part as u32)), Some(folder));
        assert!(run(
            &mut dom,
            &mut sel,
            "instance.new",
            json!({"class": "NotAClass"})
        )
        .is_err());

        let database = database();
        let mut host = DomHost {
            dom: &mut dom,
            database: &database,
            selection: &mut sel,
            selection_changed: false,
        };
        host.call("selection.set", &json!({"refs": [part]}))
            .unwrap();
        assert!(host.selection_changed);
        assert_eq!(
            host.call("selection.get", &json!({})).unwrap(),
            json!([part])
        );
        host.call("instance.destroy", &json!({"ref": part}))
            .unwrap();
        assert_eq!(host.call("selection.get", &json!({})).unwrap(), json!([]));
        assert!(host.dom.get(Ref::new(part as u32)).is_none());
    }
}
