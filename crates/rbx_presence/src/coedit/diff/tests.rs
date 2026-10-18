use super::*;
use crate::coedit::Batch;
use rbx_dom::Vector3Data;

struct Fixture {
    dom: WeakDom,
    ws: Ref,
    part: Ref,
    script: Ref,
}

fn fixture() -> Fixture {
    let mut dom = WeakDom::new();
    let ws = dom.new_instance("Workspace", "Workspace", None);
    let part = dom.new_instance("Part", "P", Some(ws));
    dom.set_property(part, "Anchored", Variant::Bool(true))
        .unwrap();
    dom.set_property(
        part,
        "Size",
        Variant::Vector3(Vector3Data {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        }),
    )
    .unwrap();
    let script = dom.new_instance("Script", "S", Some(ws));
    dom.set_property(script, "Source", Variant::String("a\n".into()))
        .unwrap();
    dom.take_changes();
    Fixture {
        dom,
        ws,
        part,
        script,
    }
}

fn diff(shadow: &WeakDom, current: &mut WeakDom) -> (Vec<Op>, IdMap) {
    let mut ids = IdMap::build(shadow);
    let mut eng = Engine::new("1:studio".into());
    let ops = local_ops(shadow, current, &mut ids, &mut eng);
    (ops, ids)
}

fn dump(dom: &WeakDom, r: Ref) -> String {
    let i = dom.get(r).unwrap();
    let props: std::collections::BTreeMap<_, _> = i
        .properties()
        .iter()
        .filter(|(k, _)| k.as_str() != "UniqueId")
        .collect();
    let mut kids: Vec<String> = i.children().iter().map(|&c| dump(dom, c)).collect();
    kids.sort();
    format!("{}:{}{:?}[{}]", i.name(), i.class(), props, kids.join(","))
}

fn dump_all(dom: &WeakDom) -> Vec<String> {
    let mut v: Vec<String> = dom.root_refs().iter().map(|&r| dump(dom, r)).collect();
    v.sort();
    v
}

#[test]
fn no_change_no_ops() {
    let f = fixture();
    let mut cur = f.dom.clone();
    assert!(diff(&f.dom, &mut cur).0.is_empty());
}

#[test]
fn rename_and_reparent() {
    let f = fixture();
    let mut cur = f.dom.clone();
    cur.set_name(f.part, "Q").unwrap();
    let ops = diff(&f.dom, &mut cur).0;
    assert_eq!(ops.len(), 1);
    assert!(matches!(&ops[0], Op::Set { n, .. } if n == "Name"));

    let mut cur = f.dom.clone();
    cur.set_parent(f.part, Some(f.script));
    let ops = diff(&f.dom, &mut cur).0;
    assert_eq!(ops.len(), 1);
    match &ops[0] {
        Op::Set { n, v, at, .. } => {
            assert_eq!(n, "Parent");
            assert_eq!(v["v"]["p"], "Workspace.S");
            assert_eq!(at.p, "Workspace.S.P");
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn property_set_and_remove() {
    let f = fixture();
    let mut cur = f.dom.clone();
    cur.set_property(f.part, "Anchored", Variant::Bool(false))
        .unwrap();
    cur.remove_property(f.part, "Size").unwrap();
    let ops = diff(&f.dom, &mut cur).0;
    assert_eq!(ops.len(), 2);
    assert!(matches!(&ops[0], Op::Set { n, v, .. } if n == "Anchored" && v["v"] == false));
    assert!(matches!(&ops[1], Op::Set { n, v, .. } if n == "Size" && v.is_null()));
}

#[test]
fn add_subtree_parent_first() {
    let f = fixture();
    let mut cur = f.dom.clone();
    let m = cur.new_instance("Model", "M", Some(f.ws));
    let c = cur.new_instance("Part", "C", Some(m));
    cur.set_property(c, "Anchored", Variant::Bool(true))
        .unwrap();
    let ops = diff(&f.dom, &mut cur).0;
    assert_eq!(ops.len(), 2);
    match (&ops[0], &ops[1]) {
        (
            Op::Add {
                cls, props, parent, ..
            },
            Op::Add {
                props: cp,
                parent: cparent,
                ..
            },
        ) => {
            assert_eq!(cls, "Model");
            assert_eq!(props["Name"]["v"], "M");
            assert_eq!(parent.as_ref().unwrap().p, "Workspace");
            assert_eq!(cp["Name"]["v"], "C");
            assert_eq!(cp["Anchored"]["v"], true);
            assert_eq!(cparent.as_ref().unwrap().p, "Workspace.M");
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn remove_subtree_one_del() {
    let mut f = fixture();
    let m = f.dom.new_instance("Model", "M", Some(f.ws));
    f.dom.new_instance("Part", "C", Some(m));
    let mut cur = f.dom.clone();
    cur.remove(m);
    let ops = diff(&f.dom, &mut cur).0;
    assert_eq!(ops.len(), 1);
    assert!(matches!(&ops[0], Op::Del { at, .. } if at.p == "Workspace.M"));
}

#[test]
fn class_change_del_then_add() {
    let f = fixture();
    let mut cur = f.dom.clone();
    let _ = cur.set_class(f.part, "Model");
    let ops = diff(&f.dom, &mut cur).0;
    assert!(matches!(&ops[0], Op::Del { .. }));
    assert!(matches!(&ops[1], Op::Add { cls, .. } if cls == "Model"));
}

#[test]
fn script_edit_is_src_against_old_text() {
    let f = fixture();
    let mut cur = f.dom.clone();
    cur.set_property(f.script, "Source", Variant::String("b\n".into()))
        .unwrap();
    let ops = diff(&f.dom, &mut cur).0;
    assert_eq!(ops.len(), 1);
    match &ops[0] {
        Op::Src { base, text, .. } => {
            assert_eq!(*base, crate::coedit::merge::text_hash("a\n"));
            assert_eq!(text, "b\n");
        }
        other => panic!("unexpected {other:?}"),
    }
}

fn converge(shadow: &WeakDom, current: &mut WeakDom) {
    let (ops, ids) = diff(shadow, current);
    let mut target = shadow.clone();
    let mut ids = ids.clone();
    let mut eng = Engine::new("2:rbx-native".into());
    let batch = Batch {
        a: "1:studio".into(),
        ops,
    };
    eng.apply(&mut target, &mut ids, &batch);
    assert_eq!(dump_all(&target), dump_all(current));
}

#[test]
fn round_trip_converges() {
    let f = fixture();
    let mut cur = f.dom.clone();
    cur.set_name(f.part, "Q").unwrap();
    cur.set_property(f.part, "Anchored", Variant::Bool(false))
        .unwrap();
    cur.remove_property(f.part, "Size").unwrap();
    cur.set_property(f.script, "Source", Variant::String("b\n".into()))
        .unwrap();
    let m = cur.new_instance("Model", "M", Some(f.ws));
    let c = cur.new_instance("Part", "C", Some(m));
    cur.set_property(c, "Anchored", Variant::Bool(true))
        .unwrap();
    cur.set_parent(f.part, Some(m));
    converge(&f.dom, &mut cur);
}

#[test]
fn undo_reverts() {
    let f = fixture();
    let mut modified = f.dom.clone();
    modified.set_name(f.part, "Q").unwrap();
    modified
        .set_property(f.part, "Anchored", Variant::Bool(false))
        .unwrap();
    modified.new_instance("Part", "New", Some(f.ws));
    let mut older = f.dom.clone();
    converge(&modified, &mut older);
}
