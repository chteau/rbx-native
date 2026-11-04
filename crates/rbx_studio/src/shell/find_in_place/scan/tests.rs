use std::collections::HashMap;

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::*;
use crate::history::{History, DEFAULT_CAP};
use crate::script_editor::find::Options;

/// Three scripts under two services: a server `Main`, a shared `Util`
/// module and a client script, plus a part that has no `Source`.
fn place() -> (WeakDom, [Ref; 3]) {
    let mut dom = WeakDom::new();
    let server = dom.new_instance("ServerScriptService", "ServerScriptService", None);
    let shared = dom.new_instance("ReplicatedStorage", "ReplicatedStorage", None);
    dom.new_instance("Part", "Part", Some(shared));
    let make = |dom: &mut WeakDom, class: &str, name: &str, parent: Ref, source: &str| {
        let script = dom.new_instance(class, name, Some(parent));
        let _ = dom.set_property(script, "Source", Variant::String(source.to_owned()));
        script
    };
    let main = make(
        &mut dom,
        "Script",
        "Main",
        server,
        "local foo = 1\nprint(foo, Foo)\n",
    );
    let util = make(
        &mut dom,
        "ModuleScript",
        "Util",
        shared,
        "return { foo = foo }\n",
    );
    let client = make(
        &mut dom,
        "LocalScript",
        "Client",
        shared,
        "print('no match')\n",
    );
    (dom, [main, util, client])
}

fn pattern(query: &str) -> Pattern {
    Pattern::compile(query, Options::default()).unwrap()
}

#[test]
fn every_script_is_searched_open_or_not() {
    let (dom, [main, util, _]) = place();
    let database = ReflectionDatabase::embedded();
    let typing = HashMap::new();
    let place = Place {
        dom: &dom,
        database: &database,
        typing: &typing,
    };
    let found = scan(&place, &pattern("foo"), "", None);
    assert_eq!(found.total, 5);
    let names: Vec<_> = found
        .files
        .iter()
        .map(|f| (f.script, f.name.as_str(), f.count))
        .collect();
    assert_eq!(
        names,
        [
            (main, "ServerScriptService.Main", 3),
            (util, "ReplicatedStorage.Util", 2),
        ]
    );
    let hit = &found.files[0].hits[1];
    assert_eq!(
        (
            hit.line,
            hit.before.as_str(),
            hit.text.as_str(),
            hit.after.as_str()
        ),
        (2, "print(", "foo", ", Foo)")
    );
}

#[test]
fn an_open_tabs_typing_wins_over_the_dom() {
    let (dom, [main, ..]) = place();
    let database = ReflectionDatabase::embedded();
    let typing = HashMap::from([(main, "nothing here".to_owned())]);
    let place = Place {
        dom: &dom,
        database: &database,
        typing: &typing,
    };
    let found = scan(&place, &pattern("foo"), "", None);
    assert_eq!(found.files.len(), 1);
    assert_eq!(found.files[0].name, "ReplicatedStorage.Util");
}

#[test]
fn the_where_filter_keeps_scripts_by_full_name() {
    let (dom, _) = place();
    let database = ReflectionDatabase::embedded();
    let typing = HashMap::new();
    let place = Place {
        dom: &dom,
        database: &database,
        typing: &typing,
    };
    let found = scan(&place, &pattern("foo"), " main", None);
    assert_eq!(found.files.len(), 1);
    assert_eq!(found.files[0].name, "ServerScriptService.Main");
    assert_eq!(
        scan(&place, &pattern("foo"), "replicatedstorage.util", None).total,
        2
    );
    assert_eq!(scan(&place, &pattern("foo"), "nope", None).total, 0);
}

#[test]
fn the_preview_carries_the_expanded_replacement() {
    let (dom, _) = place();
    let database = ReflectionDatabase::embedded();
    let typing = HashMap::new();
    let place = Place {
        dom: &dom,
        database: &database,
        typing: &typing,
    };
    let regex = Pattern::compile(
        r"(\w+) = (\d)",
        Options {
            regex: true,
            ..Options::default()
        },
    )
    .unwrap();
    let found = scan(&place, &regex, "", Some("$2 -> $1"));
    assert_eq!(
        found.files[0].hits[0].replacement.as_deref(),
        Some("1 -> foo")
    );
}

#[test]
fn the_list_is_capped_but_the_count_is_not() {
    let mut dom = WeakDom::new();
    let root = dom.new_instance("ServerScriptService", "ServerScriptService", None);
    let script = dom.new_instance("Script", "Big", Some(root));
    let _ = dom.set_property(
        script,
        "Source",
        Variant::String("a\n".repeat(MAX_HITS + 20)),
    );
    let database = ReflectionDatabase::embedded();
    let typing = HashMap::new();
    let place = Place {
        dom: &dom,
        database: &database,
        typing: &typing,
    };
    let found = scan(&place, &pattern("a"), "", None);
    assert_eq!(found.total, MAX_HITS + 20);
    assert_eq!(found.files[0].count, MAX_HITS + 20);
    assert_eq!(found.files[0].hits.len(), MAX_HITS);
}

#[test]
fn replace_all_over_open_and_closed_scripts_is_one_undo_step() {
    let (mut dom, [main, util, client]) = place();
    let database = ReflectionDatabase::embedded();
    let typing = HashMap::new();
    let edits = replacements(
        &Place {
            dom: &dom,
            database: &database,
            typing: &typing,
        },
        &pattern("foo"),
        "",
        "bar",
    );
    assert_eq!(edits.len(), 2);

    // What `Shell::write_script_edits` does around the writes.
    let mut history = History::new(DEFAULT_CAP);
    dom.take_changes();
    let before = dom.clone();
    assert!(write_all(&mut dom, &edits));
    history.push(before);

    assert_eq!(
        source::read(&dom, main).unwrap(),
        "local bar = 1\nprint(bar, bar)\n"
    );
    assert_eq!(source::read(&dom, util).unwrap(), "return { bar = bar }\n");
    assert_eq!(source::read(&dom, client).unwrap(), "print('no match')\n");

    let (restored, _) = history.undo(dom).expect("one step to undo");
    assert_eq!(
        source::read(&restored, main).unwrap(),
        "local foo = 1\nprint(foo, Foo)\n"
    );
    assert_eq!(
        source::read(&restored, util).unwrap(),
        "return { foo = foo }\n"
    );
    assert!(
        history.undo(restored).is_none(),
        "everything was a single step"
    );
}

#[test]
fn writing_nothing_new_reports_no_change() {
    let (mut dom, [main, ..]) = place();
    let same = source::read(&dom, main).unwrap();
    assert!(!write_all(&mut dom, &[(main, same)]));
}
