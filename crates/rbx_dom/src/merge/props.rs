//! Merges what one instance holds: its name, class, properties,
//! attributes, tags and, for a script, its `Source`.

mod attrs;

use std::collections::BTreeMap;

use super::text::{merge_text, TextChunk};
use super::{Resolution, Side, Target};
use crate::attributes::{encode_tags, tags};
use crate::{Instance, Ref, Variant};

/// One instance on each side that has it.
pub(crate) struct Sides<'a> {
    pub(crate) base: Option<&'a Instance>,
    pub(crate) ours: Option<&'a Instance>,
    pub(crate) theirs: Option<&'a Instance>,
}

/// An instance's merged content; every `Ref` (property or `Instance`
/// attribute) already names an output referent.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Content {
    pub(crate) name: String,
    pub(crate) class: String,
    pub(crate) properties: BTreeMap<String, Variant>,
}

/// A conflict found in one instance's content; values as `Conflict`
/// documents them.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Found {
    pub(crate) target: Target,
    pub(crate) base: Option<Variant>,
    pub(crate) ours: Option<Variant>,
    pub(crate) theirs: Option<Variant>,
}

enum Pick<T> {
    Agreed(T),
    Conflict,
}

fn three<T: PartialEq>(has_base: bool, b: T, o: T, t: T) -> Pick<T> {
    if o == t {
        Pick::Agreed(o)
    } else if has_base && o == b {
        Pick::Agreed(t)
    } else if has_base && t == b {
        Pick::Agreed(o)
    } else {
        Pick::Conflict
    }
}

/// The side a non-custom resolution picks; `None` is ours.
fn settle_side<T>(r: Option<Resolution>, b: T, o: T, t: T) -> T {
    match r {
        Some(Resolution::Base) => b,
        Some(Resolution::Theirs) => t,
        _ => o,
    }
}

type Slot<'a> = (Side, &'a Instance);

struct Ctx<'a, 'f> {
    base: Option<Slot<'a>>,
    ours: Slot<'a>,
    theirs: Slot<'a>,
    out: &'f dyn Fn(Side, Ref) -> Option<Ref>,
    choose: &'f dyn Fn(&Target) -> Option<Resolution>,
    found: Vec<Found>,
}

const SCRIPTS: [&str; 3] = ["Script", "LocalScript", "ModuleScript"];

impl<'a> Ctx<'a, '_> {
    fn has_base(&self) -> bool {
        self.base.is_some()
    }

    fn value(
        &mut self,
        target: Target,
        b: Option<Variant>,
        o: Option<Variant>,
        t: Option<Variant>,
    ) -> Option<Variant> {
        match three(self.has_base(), &b, &o, &t) {
            Pick::Agreed(v) => v.clone(),
            Pick::Conflict => {
                let resolution = (self.choose)(&target);
                self.found.push(Found {
                    target,
                    base: b.clone(),
                    ours: o.clone(),
                    theirs: t.clone(),
                });
                match resolution {
                    Some(Resolution::Custom(v)) => Some(v),
                    r => settle_side(r, b, o, t),
                }
            }
        }
    }

    fn string(&mut self, target: Target, get: fn(&Instance) -> &str) -> String {
        let s = |slot: Option<Slot<'_>>| slot.map(|(_, i)| Variant::String(get(i).to_string()));
        let ours = get(self.ours.1).to_string();
        match self.value(
            target,
            s(self.base),
            s(Some(self.ours)),
            s(Some(self.theirs)),
        ) {
            Some(Variant::String(v)) => v,
            _ => ours,
        }
    }

    fn prop(&self, slot: Option<Slot<'_>>, key: &str) -> Option<Variant> {
        let (side, inst) = slot?;
        match inst.properties().get(key)? {
            Variant::Ref(r) => (self.out)(side, *r).map(Variant::Ref),
            v => Some(v.clone()),
        }
    }

    fn tags(&self) -> Option<Variant> {
        fn get<'a>(slot: Option<Slot<'a>>) -> Option<Vec<&'a str>> {
            slot.map(|(_, i)| tags(i.properties().get("Tags")))
        }
        let base = get(self.base).unwrap_or_default();
        let ours = get(Some(self.ours)).unwrap_or_default();
        let theirs = get(Some(self.theirs)).unwrap_or_default();
        let mut merged: Vec<&str> = ours
            .iter()
            .copied()
            .filter(|tag| !base.contains(tag) || theirs.contains(tag))
            .collect();
        for tag in &theirs {
            if !base.contains(tag) && !merged.contains(tag) {
                merged.push(*tag);
            }
        }
        (!merged.is_empty()).then(|| Variant::String(encode_tags(&merged)))
    }

    fn attributes(&mut self) -> Option<Option<Variant>> {
        let key = "AttributesSerialize";
        let out = self.out;
        let read = |slot: Slot<'a>| attrs::read(slot.0, slot.1.properties().get(key), out);
        let base = match self.base {
            Some(slot) => Some(read(slot)?),
            None => None,
        };
        let ours = read(self.ours)?;
        let theirs = read(self.theirs)?;
        Some(attrs::merge(
            base.as_ref(),
            &ours,
            &theirs,
            self.choose,
            &mut self.found,
        ))
    }

    fn source(&mut self) -> Option<Option<Variant>> {
        if !SCRIPTS.contains(&self.ours.1.class()) {
            return None;
        }
        let text = |slot: Slot<'_>| match slot.1.properties().get("Source") {
            Some(Variant::String(s)) => Some(s.clone()),
            _ => None,
        };
        let base = match self.base {
            Some(slot) => Some(text(slot)?),
            None => None,
        };
        let ours = text(self.ours)?;
        let theirs = text(self.theirs)?;
        let mut merged = String::new();
        let mut n = 0;
        let chunks = merge_text(base.as_deref().unwrap_or(""), &ours, &theirs);
        for chunk in chunks {
            match chunk {
                TextChunk::Clean(s) => merged.push_str(&s),
                TextChunk::Conflict {
                    base: b,
                    ours: o,
                    theirs: t,
                } => {
                    let target = Target::Source(n);
                    n += 1;
                    let resolution = (self.choose)(&target);
                    let lines = match &resolution {
                        Some(Resolution::Custom(Variant::String(s))) => s.as_str(),
                        Some(Resolution::Custom(_)) => o.as_str(),
                        r => settle_side(r.clone(), b.as_str(), o.as_str(), t.as_str()),
                    };
                    merged.push_str(lines);
                    self.found.push(Found {
                        target,
                        base: self.has_base().then(|| Variant::String(b.clone())),
                        ours: Some(Variant::String(o)),
                        theirs: Some(Variant::String(t)),
                    });
                }
            }
        }
        Some(Some(Variant::String(merged)))
    }
}

/// Merges `sides`. `out(side, r)` translates a referent of `side` into the
/// output (`None`: the instance it names is not in the output, and the
/// value is dropped). `choose(target)` is the resolution of that conflict,
/// `None` meaning ours.
///
/// Rule per value (absent = `None`, `Ref`s compared after translation):
/// ours == theirs → ours; ours == base → theirs; theirs == base → ours;
/// otherwise a `Found`, settled by `choose`. A side missing the instance
/// altogether contributes nothing: with one side present its content is
/// taken whole; with ours and theirs but no base (both added the same
/// instance) every difference is a conflict with `base: None`.
/// - Name → `Target::Name`, class → `Target::Class` (`Variant::String`s).
/// - `UniqueId` is ours' when ours has the instance, else theirs'; never a
///   conflict.
/// - `Tags` merge as a set (`attributes::tags`/`encode_tags`): ours' tags
///   minus those theirs removed from base, plus those theirs added, ours'
///   order first; never a conflict; an empty set drops the property.
/// - `AttributesSerialize` merges per attribute (`attributes::split_entries`)
///   under `Target::Attribute(name)`; the output blob is a `u32` count plus
///   the chosen raw entries, ours' order first, an `Instance` entry's last
///   4 bytes rewritten to the translated referent; kept as the side's
///   blob verbatim when every entry came from that side unchanged.
/// - `Source` on Script/LocalScript/ModuleScript, when all present sides
///   hold `Variant::String`, merges with `merge_text`; each conflicting
///   chunk is `Target::Source(n)` (values: its lines), and the output text
///   is clean chunks plus each conflict's chosen lines. Otherwise `Source`
///   is an ordinary property.
/// - Every other key → `Target::Property(name)`.
///
/// `Resolution::Base`/`Theirs`/`Ours` pick that side's value; `Custom`
/// is used as given.
pub(crate) fn merge_content(
    sides: &Sides<'_>,
    out: &dyn Fn(Side, Ref) -> Option<Ref>,
    choose: &dyn Fn(&Target) -> Option<Resolution>,
) -> (Content, Vec<Found>) {
    let (base, ours, theirs) = match (sides.ours, sides.theirs) {
        (Some(o), Some(t)) => (
            sides.base.map(|b| (Side::Base, b)),
            (Side::Ours, o),
            (Side::Theirs, t),
        ),
        _ => {
            let only = sides
                .ours
                .map(|i| (Side::Ours, i))
                .or(sides.theirs.map(|i| (Side::Theirs, i)))
                .or(sides.base.map(|i| (Side::Base, i)));
            let Some(slot) = only else {
                let empty = Content {
                    name: String::new(),
                    class: String::new(),
                    properties: BTreeMap::new(),
                };
                return (empty, Vec::new());
            };
            (Some(slot), slot, slot)
        }
    };
    let mut ctx = Ctx {
        base,
        ours,
        theirs,
        out,
        choose,
        found: Vec::new(),
    };

    let name = ctx.string(Target::Name, Instance::name);
    let class = ctx.string(Target::Class, Instance::class);
    let mut properties = BTreeMap::new();
    if let Some(id) = ctx.ours.1.properties().get("UniqueId") {
        properties.insert("UniqueId".to_string(), id.clone());
    }
    if let Some(t) = ctx.tags() {
        properties.insert("Tags".to_string(), t);
    }
    let mut special: Vec<&str> = vec!["UniqueId", "Tags"];
    for (key, merged) in [
        ("AttributesSerialize", ctx.attributes()),
        ("Source", ctx.source()),
    ] {
        if let Some(value) = merged {
            special.push(key);
            if let Some(v) = value {
                properties.insert(key.to_string(), v);
            }
        }
    }

    let mut keys: Vec<String> = Vec::new();
    for (_, inst) in ctx.base.into_iter().chain([ctx.ours, ctx.theirs]) {
        for key in inst.properties().keys() {
            if !special.contains(&key.as_str()) && !keys.contains(key) {
                keys.push(key.clone());
            }
        }
    }
    keys.sort();
    for key in keys {
        let b = ctx.prop(ctx.base, &key);
        let o = ctx.prop(Some(ctx.ours), &key);
        let t = ctx.prop(Some(ctx.theirs), &key);
        if let Some(v) = ctx.value(Target::Property(key.clone()), b, o, t) {
            properties.insert(key, v);
        }
    }

    let content = Content {
        name,
        class,
        properties,
    };
    (content, ctx.found)
}
