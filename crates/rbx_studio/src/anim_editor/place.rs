//! Where the Animation Editor finds things in a place, and where it puts
//! them: the rig a selection belongs to, and the folders Roblox's own editor
//! saves into (`ServerStorage.RBX_ANIMSAVES.<rig name>`, with an `AnimSaves`
//! `ObjectValue` on the rig pointing at it).

use rbx_animation::Rig;
use rbx_dom::{Ref, Variant, WeakDom};

const SAVES: &str = "RBX_ANIMSAVES";

/// The rig `selected` belongs to: the nearest ancestor (or itself) that is a
/// `Model` with at least one animatable joint under it.
pub(crate) fn rig_of(dom: &WeakDom, selected: Ref) -> Option<Ref> {
    let mut at = Some(selected);
    while let Some(r) = at {
        let instance = dom.get(r)?;
        if instance.class() == "Model" && !Rig::read(dom, r).is_empty() {
            return Some(r);
        }
        at = dom.parent(r);
    }
    None
}

fn child(dom: &WeakDom, of: Ref, name: &str) -> Option<Ref> {
    dom.get(of)?
        .children()
        .iter()
        .copied()
        .find(|&c| dom.get(c).is_some_and(|i| i.name() == name))
}

fn root_service(dom: &WeakDom, class: &str) -> Option<Ref> {
    dom.root_refs()
        .iter()
        .copied()
        .find(|&r| dom.get(r).is_some_and(|i| i.class() == class))
}

/// The rig's save folder, made (with `ServerStorage` itself if the place
/// has none) when missing.
pub(crate) fn save_folder(dom: &mut WeakDom, rig: Ref) -> Ref {
    let storage = root_service(dom, "ServerStorage")
        .unwrap_or_else(|| dom.new_instance("ServerStorage", "ServerStorage", None));
    let saves = child(dom, storage, SAVES)
        .unwrap_or_else(|| dom.new_instance("Folder", SAVES, Some(storage)));
    let name = dom
        .get(rig)
        .map(|i| i.name().to_owned())
        .unwrap_or_default();
    let folder = child(dom, saves, &name)
        .filter(|&f| dom.get(f).is_some_and(|i| i.class() == "Folder"))
        .unwrap_or_else(|| dom.new_instance("Folder", &name, Some(saves)));
    let pointer = child(dom, rig, "AnimSaves")
        .filter(|&p| dom.get(p).is_some_and(|i| i.class() == "ObjectValue"))
        .unwrap_or_else(|| dom.new_instance("ObjectValue", "AnimSaves", Some(rig)));
    let _ = dom.set_property(pointer, "Value", Variant::Ref(folder));
    folder
}

fn descendants_of_class(dom: &WeakDom, of: Ref, class: &str, out: &mut Vec<Ref>) {
    let Some(instance) = dom.get(of) else { return };
    for &c in instance.children() {
        if dom.get(c).is_some_and(|i| i.class() == class) {
            out.push(c);
        }
        descendants_of_class(dom, c, class, out);
    }
}

/// Every instance of `class` in the place: the editor's Load list, with a
/// rig's own saves first.
pub(crate) fn all_of_class(dom: &WeakDom, class: &str, rig: Option<Ref>) -> Vec<(Ref, String)> {
    let mut found = Vec::new();
    for &root in dom.root_refs() {
        if dom.get(root).is_some_and(|i| i.class() == class) {
            found.push(root);
        }
        descendants_of_class(dom, root, class, &mut found);
    }
    let preferred = rig.and_then(|r| child(dom, r, "AnimSaves")).and_then(|p| {
        match dom.get(p)?.properties().get("Value")? {
            Variant::Ref(f) => Some(*f),
            _ => None,
        }
    });
    let in_folder = |r: Ref| preferred.is_some() && dom.parent(r) == preferred;
    found.sort_by_key(|&r| !in_folder(r));
    found
        .into_iter()
        .map(|r| {
            (
                r,
                dom.get(r).map(|i| i.name().to_owned()).unwrap_or_default(),
            )
        })
        .collect()
}

/// The editor's Load list: every `KeyframeSequence` and `CurveAnimation`,
/// the rig's own saves first.
pub(crate) fn saved_animations(
    dom: &WeakDom,
    rig: Option<Ref>,
) -> Vec<(Ref, String, &'static str)> {
    let mut all: Vec<(Ref, String, &'static str)> = Vec::new();
    for class in ["KeyframeSequence", "CurveAnimation"] {
        all.extend(
            all_of_class(dom, class, rig)
                .into_iter()
                .map(|(r, n)| (r, n, class)),
        );
    }
    let preferred = rig.and_then(|r| child(dom, r, "AnimSaves")).and_then(|p| {
        match dom.get(p)?.properties().get("Value")? {
            Variant::Ref(f) => Some(*f),
            _ => None,
        }
    });
    all.sort_by_key(|(r, _, _)| !(preferred.is_some() && dom.parent(*r) == preferred));
    all
}

/// A `KeyframeSequence` named `name`, preferring the rig's own saves: how a
/// graph's Clip node finds its animation without a published asset id.
pub(crate) fn sequence_named(dom: &WeakDom, name: &str, rig: Option<Ref>) -> Option<Ref> {
    let name = name.trim().trim_start_matches("rbxassetid://");
    all_of_class(dom, "KeyframeSequence", rig)
        .into_iter()
        .find(|(_, n)| n == name)
        .map(|(r, _)| r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place() -> (WeakDom, Ref, Ref) {
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        let model = dom.new_instance("Model", "Dummy", Some(workspace));
        let a = dom.new_instance("Part", "HumanoidRootPart", Some(model));
        let b = dom.new_instance("Part", "Torso", Some(model));
        let m = dom.new_instance("Motor6D", "Root", Some(b));
        dom.set_property(m, "Part0", Variant::Ref(a)).unwrap();
        dom.set_property(m, "Part1", Variant::Ref(b)).unwrap();
        (dom, model, m)
    }

    #[test]
    fn finds_the_rig_from_anything_inside_it() {
        let (dom, model, motor) = place();
        assert_eq!(rig_of(&dom, motor), Some(model));
        assert_eq!(rig_of(&dom, model), Some(model));
        assert_eq!(rig_of(&dom, dom.root_refs()[0]), None);
    }

    #[test]
    fn save_folder_is_made_once_and_pointed_at() {
        let (mut dom, model, _) = place();
        let folder = save_folder(&mut dom, model);
        assert_eq!(save_folder(&mut dom, model), folder);
        let storage = root_service(&dom, "ServerStorage").unwrap();
        let saves = child(&dom, storage, SAVES).unwrap();
        assert_eq!(dom.parent(folder), Some(saves));
        let pointer = child(&dom, model, "AnimSaves").unwrap();
        assert_eq!(
            dom.get(pointer).unwrap().properties()["Value"],
            Variant::Ref(folder)
        );
    }

    #[test]
    fn saved_animations_lists_both_classes_rig_saves_first() {
        let (mut dom, model, _) = place();
        let loose = dom.new_instance("KeyframeSequence", "Loose", Some(model));
        let folder = save_folder(&mut dom, model);
        let saved = dom.new_instance("CurveAnimation", "Wave", Some(folder));
        assert_eq!(
            saved_animations(&dom, Some(model)),
            [
                (saved, "Wave".to_owned(), "CurveAnimation"),
                (loose, "Loose".to_owned(), "KeyframeSequence"),
            ]
        );
    }

    #[test]
    fn lists_saves_first_and_finds_by_name() {
        let (mut dom, model, _) = place();
        let loose = dom.new_instance("KeyframeSequence", "Loose", Some(model));
        let folder = save_folder(&mut dom, model);
        let saved = dom.new_instance("KeyframeSequence", "Wave", Some(folder));
        let list = all_of_class(&dom, "KeyframeSequence", Some(model));
        assert_eq!(
            list.iter().map(|(r, _)| *r).collect::<Vec<_>>(),
            [saved, loose]
        );
        assert_eq!(
            sequence_named(&dom, "rbxassetid://Wave", Some(model)),
            Some(saved)
        );
        assert_eq!(sequence_named(&dom, "Nope", None), None);
    }
}
