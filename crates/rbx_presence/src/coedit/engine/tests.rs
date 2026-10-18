use super::*;
use serde_json::json;

fn t(id: &str, p: &str) -> Target {
    Target {
        id: id.into(),
        p: p.into(),
    }
}

fn s(v: &str) -> Value {
    json!({"t": "String", "v": v})
}

fn world() -> (WeakDom, IdMap, Ref, Ref) {
    let mut dom = WeakDom::new();
    let ws = dom.new_instance("Workspace", "Workspace", None);
    let part = dom.new_instance("Part", "Part", Some(ws));
    let script = dom.new_instance("Script", "S", Some(ws));
    dom.set_property(script, "Source", Variant::String("a\nb\nc\n".into()))
        .unwrap();
    dom.take_changes();
    let ids = IdMap::build(&dom);
    (dom, ids, part, script)
}

fn id(ids: &IdMap, r: Ref) -> String {
    ids.known(r).unwrap().to_string()
}

fn batch(a: &str, ops: Vec<Op>) -> Batch {
    Batch { a: a.into(), ops }
}

fn prop(dom: &WeakDom, r: Ref, n: &str) -> Option<Variant> {
    dom.get(r)?.properties().get(n).cloned()
}

fn source(dom: &WeakDom, r: Ref) -> String {
    source_of(dom, Some(r))
}

fn set(at: Target, n: &str, v: Value, c: Clock) -> Op {
    Op::Set {
        at,
        n: n.into(),
        v,
        c,
    }
}

#[test]
fn crossed_clocks_converge_and_older_skips() {
    let (dom0, ids0, part, _) = world();
    let pid = id(&ids0, part);
    let a = batch(
        "1:studio",
        vec![set(t(&pid, ""), "Tag", s("a"), Clock(5, "1:studio".into()))],
    );
    let b = batch(
        "2:studio",
        vec![set(t(&pid, ""), "Tag", s("b"), Clock(5, "2:studio".into()))],
    );
    let mut results = Vec::new();
    for order in [[&a, &b], [&b, &a]] {
        let (mut dom, mut ids) = (dom0.clone(), ids0.clone());
        let mut e = Engine::new("9:studio".into());
        for x in order {
            e.apply(&mut dom, &mut ids, x);
        }
        results.push(prop(&dom, part, "Tag"));
    }
    assert_eq!(results[0], results[1]);
    assert_eq!(results[0], Some(Variant::String("b".into())));

    let (mut dom, mut ids) = (dom0.clone(), ids0.clone());
    let mut e = Engine::new("9:studio".into());
    e.apply(&mut dom, &mut ids, &b);
    let old = batch(
        "1:studio",
        vec![set(
            t(&pid, ""),
            "Tag",
            s("old"),
            Clock(3, "1:studio".into()),
        )],
    );
    assert_eq!(e.apply(&mut dom, &mut ids, &old).applied, 0);
    assert_eq!(prop(&dom, part, "Tag"), Some(Variant::String("b".into())));
}

fn add(idv: &str, parent: Option<Target>, props: Value, c: u64) -> Op {
    Op::Add {
        at: t(idv, ""),
        cls: "Part".into(),
        parent,
        props: props.as_object().unwrap().clone(),
        c: Clock(c, "1:studio".into()),
    }
}

#[test]
fn add_set_upsert_del() {
    let (mut dom, mut ids, part, _) = world();
    let ws = dom.parent(part).unwrap();
    let wid = id(&ids, ws);
    let nid = "0000000100000002000000000000000f".to_string();
    let mut e = Engine::new("9:studio".into());
    let r = e.apply(
        &mut dom,
        &mut ids,
        &batch(
            "1:studio",
            vec![
                add(
                    &nid,
                    Some(t(&wid, "")),
                    json!({"Name": s("New"), "Tag": s("x")}),
                    1,
                ),
                set(t(&nid, ""), "Tag", s("y"), Clock(2, "1:studio".into())),
            ],
        ),
    );
    assert_eq!(r.applied, 2);
    let n = ids.get(&nid).unwrap();
    assert_eq!(dom.get(n).unwrap().name(), "New");
    assert_eq!(dom.parent(n), Some(ws));
    assert_eq!(prop(&dom, n, "Tag"), Some(Variant::String("y".into())));
    assert!(matches!(
        prop(&dom, n, "UniqueId"),
        Some(Variant::UniqueId(_))
    ));

    e.apply(
        &mut dom,
        &mut ids,
        &batch(
            "1:studio",
            vec![add(&nid, None, json!({"Name": s("Renamed")}), 3)],
        ),
    );
    assert_eq!(ids.get(&nid), Some(n));
    assert_eq!(dom.get(n).unwrap().name(), "Renamed");
    assert_eq!(dom.parent(n), None);

    e.apply(
        &mut dom,
        &mut ids,
        &batch(
            "1:studio",
            vec![Op::Del {
                at: t(&nid, ""),
                c: Clock(4, "1:studio".into()),
            }],
        ),
    );
    assert!(dom.get(n).is_none());
    let late = e.apply(
        &mut dom,
        &mut ids,
        &batch(
            "2:studio",
            vec![set(
                t(&nid, "Renamed"),
                "Tag",
                s("z"),
                Clock(9, "2:studio".into()),
            )],
        ),
    );
    assert_eq!(late.applied, 0);
    assert!(dom.get(n).is_none());
}

#[test]
fn ref_resolves_to_add_later_in_batch_and_path_fallback() {
    let (mut dom, mut ids, part, _) = world();
    let pid = id(&ids, part);
    let mut e = Engine::new("9:studio".into());
    let r = e.apply(
        &mut dom,
        &mut ids,
        &batch(
            "1:studio",
            vec![
                set(
                    t(&pid, ""),
                    "Link",
                    json!({"t": "Ref", "v": {"id": "fresh", "p": ""}}),
                    Clock(1, "1:studio".into()),
                ),
                add("fresh", None, json!({"Name": s("F")}), 2),
                set(
                    t("", "Workspace.Part"),
                    "Tag",
                    s("p"),
                    Clock(3, "1:studio".into()),
                ),
            ],
        ),
    );
    let fresh = ids.get("fresh").unwrap();
    assert_eq!(prop(&dom, part, "Link"), Some(Variant::Ref(fresh)));
    assert_eq!(prop(&dom, part, "Tag"), Some(Variant::String("p".into())));
    assert_eq!(r.applied, 2);
}

#[test]
fn parent_cycle_refused() {
    let (mut dom, mut ids, part, _) = world();
    let ws = dom.parent(part).unwrap();
    let (wid, pid) = (id(&ids, ws), id(&ids, part));
    let mut e = Engine::new("9:studio".into());
    let r = e.apply(
        &mut dom,
        &mut ids,
        &batch(
            "1:studio",
            vec![set(
                t(&wid, ""),
                "Parent",
                json!({"t": "Ref", "v": {"id": pid, "p": ""}}),
                Clock(1, "1:studio".into()),
            )],
        ),
    );
    assert_eq!(r.applied, 0);
    assert_eq!(r.notes.len(), 1);
    assert_eq!(dom.parent(ws), None);
}

fn src(sid: &str, base: &str, text: &str, c: Clock) -> Op {
    Op::Src {
        at: t(sid, "Workspace.S"),
        base: base.into(),
        text: text.into(),
        c,
    }
}

#[test]
fn src_fast_forward_and_stale_rejected() {
    let (mut dom, mut ids, _, script) = world();
    let sid = id(&ids, script);
    let mut e = Engine::new("9:studio".into());
    let h0 = e.log_text_hash(&sid, "a\nb\nc\n");
    e.apply(
        &mut dom,
        &mut ids,
        &batch(
            "1:studio",
            vec![src(&sid, &h0, "a\nB\nc\n", Clock(1, "1:studio".into()))],
        ),
    );
    assert_eq!(source(&dom, script), "a\nB\nc\n");
    let r = e.apply(
        &mut dom,
        &mut ids,
        &batch(
            "2:studio",
            vec![src(&sid, &h0, "X\nb\nc\n", Clock(2, "2:studio".into()))],
        ),
    );
    assert_eq!(r.applied, 0);
    assert!(r.resend.is_empty());
    assert_eq!(source(&dom, script), "a\nB\nc\n");
    assert_eq!(e.log_text_hash(&sid, ""), text_hash("a\nB\nc\n"));
}

#[test]
fn own_rejected_src_resends_the_merged_text_once() {
    let (mut dom, mut ids, _, script) = world();
    let sid = id(&ids, script);
    let mut e = Engine::new("1:studio".into());
    let h0 = e.log_text_hash(&sid, "a\nb\nc\n");
    // This editor's edit is on screen and in flight when another lands first.
    dom.set_property(script, "Source", Variant::String("a\nb\nC\n".into()))
        .unwrap();
    e.apply(
        &mut dom,
        &mut ids,
        &batch(
            "2:studio",
            vec![src(&sid, &h0, "A\nb\nc\n", Clock(4, "2:studio".into()))],
        ),
    );
    let r = e.apply(
        &mut dom,
        &mut ids,
        &batch(
            "1:studio",
            vec![src(&sid, &h0, "a\nb\nC\n", Clock(1, "1:studio".into()))],
        ),
    );
    assert_eq!(r.resend.len(), 1);
    match &r.resend[0] {
        Op::Src { base, text, c, .. } => {
            assert_eq!(base, &text_hash("A\nb\nc\n"));
            assert_eq!(text, "A\nb\nC\n");
            assert!(c.0 > 4);
        }
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(source(&dom, script), "A\nb\nC\n");
}

#[test]
fn local_unsent_text_merges_with_remote() {
    let (mut dom, mut ids, _, script) = world();
    let sid = id(&ids, script);
    let mut e = Engine::new("1:studio".into());
    let h0 = e.log_text_hash(&sid, "a\nb\nc\n");
    dom.set_property(script, "Source", Variant::String("a\nb\nC\n".into()))
        .unwrap();
    let r = e.apply(
        &mut dom,
        &mut ids,
        &batch(
            "2:studio",
            vec![src(&sid, &h0, "A\nb\nc\n", Clock(1, "2:studio".into()))],
        ),
    );
    assert!(r.notes.is_empty());
    assert_eq!(source(&dom, script), "A\nb\nC\n");

    let h1 = e.log_text_hash(&sid, "");
    dom.set_property(script, "Source", Variant::String("A\nmine\nC\n".into()))
        .unwrap();
    let r = e.apply(
        &mut dom,
        &mut ids,
        &batch(
            "2:studio",
            vec![src(
                &sid,
                &h1,
                "A\ntheirs\nc\n",
                Clock(2, "2:studio".into()),
            )],
        ),
    );
    assert_eq!(r.notes.len(), 1);
    assert!(r.notes[0].contains("Workspace.S"));
    assert!(source(&dom, script).contains("<<<<<<<"));
}

#[test]
fn replay_is_idempotent_and_tick_exceeds() {
    let (mut dom, mut ids, part, _) = world();
    let pid = id(&ids, part);
    let log = batch(
        "1:studio",
        vec![
            add("n1", None, json!({"Name": s("N")}), 7),
            set(t(&pid, ""), "Tag", s("v"), Clock(8, "1:studio".into())),
            Op::Del {
                at: t("n1", ""),
                c: Clock(41, "1:studio".into()),
            },
        ],
    );
    let mut e = Engine::new("9:studio".into());
    e.apply(&mut dom, &mut ids, &log);
    let once = (dom.root_refs().to_vec(), prop(&dom, part, "Tag"));
    let r = e.apply(&mut dom, &mut ids, &log);
    assert_eq!(r.applied, 0);
    assert_eq!((dom.root_refs().to_vec(), prop(&dom, part, "Tag")), once);
    assert!(e.tick().0 > 41);
}

#[test]
fn a_studio_only_property_is_not_stored() {
    let (mut dom, mut ids, part, _) = world();
    let pid = id(&ids, part);
    let mut e = Engine::new("1:rbx-native:aa".into());
    e.apply(
        &mut dom,
        &mut ids,
        &batch(
            "1:studio:bb",
            vec![set(
                t(&pid, ""),
                "BrickColor",
                json!({"t": "BrickColor", "v": 21}),
                Clock(1, "1:studio:bb".into()),
            )],
        ),
    );
    assert_eq!(prop(&dom, part, "BrickColor"), None);
}
