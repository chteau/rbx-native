//! Where a property's value lives on an instance, and bringing a file's own
//! spellings to the one Roblox saves, so every reader can look for that one.

use std::collections::HashMap;
use std::mem::discriminant;

use rbx_dom::{Instance, Variant, WeakDom};

use crate::database::ReflectionDatabase;

impl ReflectionDatabase {
    /// The value `name` holds on `instance` and the name it is stored under
    /// — whichever spelling the instance stores it as, `Size` and `size`
    /// alike — or, where it stores none, the class default and the name
    /// Roblox saves it under. `None` when there is neither.
    pub fn stored_or_default<'a>(
        &'a self,
        instance: &'a Instance,
        name: &'a str,
    ) -> Option<(&'a str, &'a Variant)> {
        let class = instance.class();
        let names = self.stored_names(class, name);
        names
            .iter()
            .find_map(|&key| Some((key, instance.properties().get(key)?)))
            .or_else(|| {
                let default = self.default_value(class, self.canonical_name(class, name))?;
                Some((*names.first()?, default))
            })
    }

    /// Renames every property `dom` holds under another spelling — `Color`,
    /// `Size`, a lowercase legacy name — to the one Roblox saves it under
    /// (`Color3uint8`, `size`), which is the one the renderer, the
    /// Properties panel and the save path all read and write. A file that
    /// holds both keeps the saved one: a place carries its order of loading
    /// no further than the parser, so there is no telling which Studio would
    /// have let win.
    ///
    /// A value is only moved when it fits the saved name's type — the one
    /// conversion is a `Color3` becoming the `Color3uint8` a part's colour
    /// is saved as — and is otherwise left where it is.
    pub fn normalize_names(&self, dom: &mut WeakDom) {
        let mut renames: HashMap<String, Vec<(&str, &str, &str)>> = HashMap::new();
        let mut stack = dom.root_refs().to_vec();
        while let Some(reference) = stack.pop() {
            let Some(instance) = dom.get_mut(reference) else {
                continue;
            };
            stack.extend(instance.children().iter().copied());
            let class = instance.class().to_owned();
            let renames = renames
                .entry(class.clone())
                .or_insert_with(|| self.renames(&class));
            let properties = instance.properties_mut();
            for &(from, to, canonical) in renames.iter() {
                let Some(value) = properties.remove(from) else {
                    continue;
                };
                if properties.contains_key(to) {
                    continue;
                }
                match conform(value, self.default_value(&class, canonical)) {
                    Ok(value) => properties.insert(to.to_owned(), value),
                    Err(value) => properties.insert(from.to_owned(), value),
                };
            }
        }
    }
}

/// `value` in the shape of `like` — the class default, which rbx-dom records
/// in the shape a file saves it in — or back unchanged when it cannot be.
fn conform(value: Variant, like: Option<&Variant>) -> Result<Variant, Variant> {
    match (value, like) {
        (Variant::Color3(color), Some(Variant::Color3uint8 { .. })) => {
            let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
            Ok(Variant::Color3uint8 {
                r: channel(color.r),
                g: channel(color.g),
                b: channel(color.b),
            })
        }
        (value, Some(like)) if discriminant(&value) != discriminant(like) => Err(value),
        (value, _) => Ok(value),
    }
}

#[cfg(test)]
mod tests {
    use rbx_dom::{Color3Data, Ref, Vector3Data};

    use super::*;

    fn database() -> ReflectionDatabase {
        let dump = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/API-Dump.json"
        ));
        let defaults = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/reflection-defaults.json"
        ));
        ReflectionDatabase::from_json_str(dump)
            .and_then(|database| database.with_defaults(defaults))
            .unwrap()
    }

    fn size() -> Variant {
        Variant::Vector3(Vector3Data {
            x: 4.0,
            y: 2.0,
            z: 1.0,
        })
    }

    fn one(class: &str, values: &[(&str, Variant)]) -> WeakDom {
        let mut dom = WeakDom::new();
        let mut instance = Instance::new(Ref::new(1), class, "It");
        for (key, value) in values {
            instance
                .properties_mut()
                .insert((*key).to_owned(), value.clone());
        }
        dom.insert(instance);
        dom
    }

    fn keys(dom: &WeakDom) -> Vec<&str> {
        let instance = dom.get(Ref::new(1)).unwrap();
        instance.properties().keys().map(String::as_str).collect()
    }

    #[test]
    fn canonical_names_become_the_saved_ones() {
        let mut dom = one(
            "Part",
            &[
                ("Size", size()),
                (
                    "Color",
                    Variant::Color3(Color3Data {
                        r: 1.0,
                        g: 0.5,
                        b: 0.0,
                    }),
                ),
                ("Shape", Variant::Enum(0)),
                ("Anchored", Variant::Bool(true)),
            ],
        );

        database().normalize_names(&mut dom);

        assert_eq!(keys(&dom), ["Anchored", "Color3uint8", "shape", "size"]);
        let part = dom.get(Ref::new(1)).unwrap();
        assert_eq!(
            part.properties()["Color3uint8"],
            Variant::Color3uint8 {
                r: 255,
                g: 128,
                b: 0
            }
        );
        assert_eq!(part.properties()["size"], size());
    }

    #[test]
    fn a_legacy_spelling_becomes_the_saved_one_too() {
        let mut dom = one("Fire", &[("size", Variant::Float32(3.0))]);

        database().normalize_names(&mut dom);

        assert_eq!(keys(&dom), ["size_xml"]);
    }

    #[test]
    fn the_saved_spelling_wins_over_another() {
        let mut dom = one(
            "Part",
            &[
                ("size", size()),
                (
                    "Size",
                    Variant::Vector3(Vector3Data {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    }),
                ),
            ],
        );

        database().normalize_names(&mut dom);

        assert_eq!(keys(&dom), ["size"]);
        assert_eq!(dom.get(Ref::new(1)).unwrap().properties()["size"], size());
    }

    #[test]
    fn a_value_of_the_wrong_type_stays_where_it_is() {
        let mut dom = one("Part", &[("Size", Variant::String("big".into()))]);

        database().normalize_names(&mut dom);

        assert_eq!(keys(&dom), ["Size"]);
    }

    #[test]
    fn another_classs_same_name_is_left_alone() {
        // A `Frame`'s `Size` is its own, and saved as `Size`.
        let mut dom = one("Frame", &[("Size", Variant::Float32(1.0))]);

        database().normalize_names(&mut dom);

        assert_eq!(keys(&dom), ["Size"]);
    }

    #[test]
    fn stored_or_default_finds_either_spelling_or_the_default() {
        let db = database();
        let dom = one("Part", &[("size", size())]);
        let part = dom.get(Ref::new(1)).unwrap();

        assert_eq!(db.stored_or_default(part, "Size"), Some(("size", &size())));
        assert_eq!(
            db.stored_or_default(part, "Anchored"),
            Some(("Anchored", &Variant::Bool(false)))
        );
        // Saved as `Color3uint8`, so that is where a default says to write.
        assert_eq!(
            db.stored_or_default(part, "Color").unwrap().0,
            "Color3uint8"
        );
        assert_eq!(db.stored_or_default(part, "Mass"), None);
    }
}
