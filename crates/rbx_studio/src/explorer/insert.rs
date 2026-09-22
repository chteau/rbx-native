//! What the Explorer's `+` picker offers under a given parent, and what a
//! freshly inserted instance is called.
//!
//! Roblox publishes no per-class table of legal parents — the API dump
//! carries class tags and nothing about ancestry, and a class page in
//! creator-docs describes where an instance *works* ("if no ancestral
//! `PVInstance` exists…") rather than where the engine refuses to put it. So
//! the picker greys exactly the two refusals the dump does state, and no
//! more:
//!
//! - **`NotCreatable`** — `Instance.new` refuses the class outright. Listed
//!   anyway, greyed, because "`Terrain` is not something you insert" is
//!   worth learning from the picker rather than from its absence.
//! - **`Service`** — a service is a singleton the `DataModel` owns and
//!   `GetService` hands out, which is why `explorer::reparent` already
//!   refuses to *drag* one anywhere. The rule takes the parent's class
//!   rather than hardcoding "never", so a `DataModel` row (which this
//!   Explorer does not show today) would still be a legal home for one.
//!
//! `NotBrowsable` classes are left out of the list entirely: that tag is
//! Roblox's own "don't show this in a class picker", so hiding them is
//! honouring the dump rather than second-guessing it.

use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

/// The one class that owns the services. Never a row in this Explorer — its
/// roots *are* the services — but the parent-legality rule is written in
/// terms of it rather than around it.
const DATA_MODEL: &str = "DataModel";

/// One row of the picker: a class, and whether inserting it under the
/// hovered parent would actually work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Choice {
    pub(crate) class: String,
    pub(crate) legal: bool,
}

/// Every class the picker lists under `parent_class` that `query` matches,
/// alphabetically. `query` is matched case-insensitively against the class
/// name as a substring, so "part" finds `MeshPart` as well as `Part`.
pub(crate) fn choices(
    database: &ReflectionDatabase,
    parent_class: Option<&str>,
    query: &str,
) -> Vec<Choice> {
    let query = query.trim().to_lowercase();
    let mut choices: Vec<Choice> = database
        .class_names()
        .filter(|class| database.is_browsable(class))
        .filter(|class| query.is_empty() || class.to_lowercase().contains(&query))
        .map(|class| Choice {
            class: class.to_owned(),
            legal: legal(database, parent_class, class),
        })
        .collect();
    // Legal first, so the classes a parent can actually take are not
    // scattered through the ones it cannot; alphabetical within each half,
    // case-insensitively, since `class_names` hands out a `HashMap`'s order.
    choices.sort_by(|left, right| {
        right
            .legal
            .cmp(&left.legal)
            .then_with(|| left.class.to_lowercase().cmp(&right.class.to_lowercase()))
    });
    choices
}

/// Whether inserting `class` under a parent of `parent_class` would work —
/// see this module's own comment for where each half of the rule comes from.
/// `None` is the file's root, which `.rbxm` places have and `.rbxl` places
/// reach only through a service.
pub(crate) fn legal(
    database: &ReflectionDatabase,
    parent_class: Option<&str>,
    class: &str,
) -> bool {
    if !database.is_creatable(class) {
        return false;
    }
    if database.is_service(class) {
        return parent_class == Some(DATA_MODEL);
    }
    true
}

/// A name for a new child of `parent` that no sibling already carries:
/// `base` itself when it is free, otherwise `base` with the lowest number
/// from 1 up that is.
///
/// This is what real Studio's **increment names for new instances** option
/// does — `studio/explorer.md`: "inserted/pasted/duplicated instances of the
/// same type will have numbered names for differentiation". With the option
/// off nothing calls this at all, and a second `Part` is simply called
/// `Part` again.
pub(crate) fn incremented_name(dom: &WeakDom, parent: Option<Ref>, base: &str) -> String {
    let siblings = match parent {
        Some(parent) => match dom.get(parent) {
            Some(instance) => instance.children(),
            None => return base.to_owned(),
        },
        None => dom.root_refs(),
    };
    let taken: Vec<&str> = siblings
        .iter()
        .filter_map(|&reference| dom.get(reference).map(|instance| instance.name()))
        .collect();

    if !taken.contains(&base) {
        return base.to_owned();
    }
    // Bounded by the sibling count: every number below the first free one is
    // taken by a distinct sibling, so the loop cannot outrun the list.
    (1..=taken.len())
        .map(|suffix| format!("{base}{suffix}"))
        .find(|candidate| !taken.iter().any(|name| *name == candidate))
        .unwrap_or_else(|| base.to_owned())
}

/// The writes that make a freshly inserted `GuiObject` something to see
/// and grab: `Instance.new`'s own values (see `reflection-defaults.json`)
/// leave it 0×0, grey, with a border. These are this editor's seed — a
/// white box, no border, text that says what it is — patterned on what
/// Studio's own UI insert shows; no docs page publishes that table. Each is
/// `properties::edit::commit` text, so an insert goes through the same
/// parser a typed value does. Empty for anything that is not a `GuiObject`.
pub(crate) fn gui_defaults(
    database: &ReflectionDatabase,
    class: &str,
) -> Vec<(&'static str, &'static str)> {
    if !database.is_subclass_of(class, "GuiObject") {
        return Vec::new();
    }
    let is = |base: &str| database.is_subclass_of(class, base);
    let size = match () {
        _ if is("TextLabel") || is("TextButton") || is("TextBox") => "0, 200, 0, 50",
        _ if is("ScrollingFrame") => "0, 200, 0, 200",
        _ => "0, 100, 0, 100",
    };
    let mut writes = vec![
        ("Size", size),
        ("BackgroundColor3", "255, 255, 255"),
        ("BorderColor3", "0, 0, 0"),
        ("BorderSizePixel", "0"),
    ];
    let text = match () {
        _ if is("TextButton") => Some("Button"),
        _ if is("TextLabel") => Some("Label"),
        _ if is("TextBox") => Some(""),
        _ => None,
    };
    if let Some(text) = text {
        writes.extend([
            ("Text", text),
            ("TextColor3", "0, 0, 0"),
            ("TextSize", "14"),
        ]);
    }
    writes
}

#[cfg(test)]
mod tests {
    use super::*;

    // A new frame is a box you can see; a new label says so; a layout or
    // a modifier is left exactly as `Instance.new` makes it.
    #[test]
    fn a_new_gui_object_is_seeded_visible_and_nothing_else_is() {
        let database = database();
        let frame = gui_defaults(&database, "Frame");
        assert!(frame.contains(&("Size", "0, 100, 0, 100")));
        assert!(frame.iter().all(|(name, _)| *name != "Text"));
        let label = gui_defaults(&database, "TextLabel");
        assert!(label.contains(&("Text", "Label")));
        assert!(label.contains(&("Size", "0, 200, 0, 50")));
        assert!(gui_defaults(&database, "UICorner").is_empty());
        assert!(gui_defaults(&database, "Part").is_empty());
    }

    fn database() -> ReflectionDatabase {
        ReflectionDatabase::embedded()
    }

    #[test]
    fn an_ordinary_class_is_legal_under_an_ordinary_parent() {
        assert!(legal(&database(), Some("Workspace"), "Part"));
        assert!(legal(&database(), Some("Folder"), "Script"));
    }

    #[test]
    fn a_class_instance_new_refuses_is_never_legal() {
        let database = database();
        assert!(!database.is_creatable("Terrain"));
        assert!(!legal(&database, Some("Workspace"), "Terrain"));
        assert!(!legal(&database, None, "Terrain"));
    }

    #[test]
    fn a_service_is_legal_only_under_the_data_model() {
        let database = database();
        assert!(database.is_service("TestService"));
        assert!(!legal(&database, Some("Workspace"), "TestService"));
        assert!(legal(&database, Some(DATA_MODEL), "TestService"));
    }

    #[test]
    fn the_picker_lists_an_illegal_class_rather_than_dropping_it() {
        let listed = choices(&database(), Some("Workspace"), "terrain");
        let terrain = listed
            .iter()
            .find(|choice| choice.class == "Terrain")
            .expect("Terrain is browsable, so the picker shows it");
        assert!(!terrain.legal);
    }

    #[test]
    fn the_picker_hides_what_roblox_marks_not_browsable() {
        let database = database();
        let hidden = database
            .class_names()
            .find(|class| !database.is_browsable(class))
            .expect("the dump tags some classes NotBrowsable")
            .to_owned();
        let listed = choices(&database, Some("Workspace"), &hidden);
        assert!(!listed.iter().any(|choice| choice.class == hidden));
    }

    #[test]
    fn a_query_matches_anywhere_in_the_name_and_ignores_case() {
        let listed = choices(&database(), Some("Workspace"), "MESHpart");
        assert!(listed.iter().any(|choice| choice.class == "MeshPart"));
    }

    #[test]
    fn legal_classes_sort_ahead_of_refused_ones() {
        let listed = choices(&database(), Some("Workspace"), "part");
        let first_illegal = listed
            .iter()
            .position(|choice| !choice.legal)
            .expect("`part` matches at least one NotCreatable class");
        assert!(listed[..first_illegal].iter().all(|choice| choice.legal));
    }

    #[test]
    fn the_first_instance_of_a_class_keeps_the_plain_name() {
        let mut dom = WeakDom::new();
        let parent = dom.new_instance("Folder", "Folder", None);
        assert_eq!(incremented_name(&dom, Some(parent), "Part"), "Part");
    }

    #[test]
    fn a_second_instance_of_a_class_is_numbered() {
        let mut dom = WeakDom::new();
        let parent = dom.new_instance("Folder", "Folder", None);
        dom.new_instance("Part", "Part", Some(parent));
        assert_eq!(incremented_name(&dom, Some(parent), "Part"), "Part1");
        dom.new_instance("Part", "Part1", Some(parent));
        assert_eq!(incremented_name(&dom, Some(parent), "Part"), "Part2");
    }

    #[test]
    fn a_gap_in_the_numbering_is_reused() {
        let mut dom = WeakDom::new();
        let parent = dom.new_instance("Folder", "Folder", None);
        dom.new_instance("Part", "Part", Some(parent));
        dom.new_instance("Part", "Part2", Some(parent));
        assert_eq!(incremented_name(&dom, Some(parent), "Part"), "Part1");
    }

    #[test]
    fn a_root_level_insert_numbers_against_the_other_roots() {
        let mut dom = WeakDom::new();
        dom.new_instance("Folder", "Folder", None);
        assert_eq!(incremented_name(&dom, None, "Folder"), "Folder1");
    }
}
