//! An edit patched in place must draw the very pixels a scene rebuilt from
//! the same DOM draws — for every kind of change `Headless::apply_changes`
//! patches, forwards, undone and redone. Anything short of that is a patch
//! that shows something a reload would not, which is the one bug this path
//! cannot afford.
//!
//! Needs a GPU, so it is `#[ignore]`d and run by hand:
//! `cargo test -p rbx_viewer --release --test patch_parity -- --ignored
//! --test-threads=1` — every test opens two or more devices of its own, and
//! eight tests' worth at once has tripped the driver into a panic deep in
//! `wgpu` before any pixel was compared. Runs against the in-repo
//! `TestPlace.rbxl` by default; `RBX_PARITY_FIXTURE` points it at a real
//! place instead (`marked.rbxl`, say).

use std::path::PathBuf;

use rbx_dom::{Change, Color3Data, Ref, UDim, UDim2, Variant, WeakDom};
use rbx_viewer::{Applied, Headless};

const SIZE: (u32, u32) = (640, 360);
/// Frames drawn before the compared one, on both sides: `Renderer::draw`
/// spreads a fresh scene's texture uploads over the frames after the first,
/// and a comparison against a half-uploaded scene compares placeholders.
const DRAIN: usize = 40;

fn fixture() -> PathBuf {
    std::env::var_os("RBX_PARITY_FIXTURE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/tests/TestPlace.rbxl")
        })
}

/// The frame `headless` draws once its uploads are through.
fn frame(headless: &mut Headless) -> Vec<u8> {
    for _ in 0..DRAIN {
        headless.render_frame(SIZE.0, SIZE.1).expect("a frame");
    }
    headless.take_frame().expect("the last frame");
    assert!(headless
        .render_frame(SIZE.0, SIZE.1)
        .expect("a frame")
        .is_none());
    headless
        .take_frame()
        .expect("the compared frame")
        .expect("one frame in flight")
        .pixels
}

/// Pixels that differ between two frames of the same size — ImageMagick's
/// `AE` metric, the number a screenshot comparison reports.
fn differing(a: &[u8], b: &[u8]) -> usize {
    assert_eq!(a.len(), b.len());
    a.chunks(4).zip(b.chunks(4)).filter(|(a, b)| a != b).count()
}

/// The frame a scene rebuilt from scratch draws for `dom`.
fn rebuilt(dom: &WeakDom) -> Vec<u8> {
    let mut reference = Headless::load(&fixture(), true).expect("the fixture loads");
    reference.reload(dom).expect("the DOM rebuilds");
    frame(&mut reference)
}

/// Applies `log` to `patched` against `dom` and checks the picture against
/// a rebuild of that same DOM.
fn check(patched: &mut Headless, dom: &WeakDom, log: &[Change], what: &str) {
    let applied = patched.apply_changes(dom, log).expect("the edit applies");
    assert_eq!(
        applied,
        Applied::Patched,
        "{what} must be patched, not rebuilt"
    );
    let ours = frame(patched);
    let theirs = rebuilt(dom);
    let ae = differing(&ours, &theirs);
    assert_eq!(
        ae, 0,
        "{what}: {ae} pixels differ from a rebuild of the same DOM"
    );
}

/// Runs one edit three ways — forwards, undone, redone — each against a
/// rebuild. Undo and redo are done the way `rbxstudio` does them: the DOM
/// from before (or after) is put back whole, and the *edit's* log is what
/// the viewport is handed.
fn parity(what: &str, edit: impl Fn(&mut WeakDom)) {
    staged(what, |_| {}, edit);
}

/// [`parity`] for an edit that needs the place set up first: `setup` is
/// patched in (and checked) as an edit of its own, so the log under test is
/// the edit's alone — a `Frame` *moved* into a `BillboardGui` is not the
/// same log as one created there.
fn staged(what: &str, setup: impl Fn(&mut WeakDom), edit: impl Fn(&mut WeakDom)) {
    let path = fixture();
    let mut dom = rbx_viewer::read_place(&path).expect("the fixture parses");
    let mut patched = Headless::load(&path, true).expect("the fixture loads");
    frame(&mut patched);

    setup(&mut dom);
    let staging = dom.take_changes();
    if !staging.is_empty() {
        check(&mut patched, &dom, &staging, &format!("{what} (setup)"));
    }

    let before = dom.clone();
    edit(&mut dom);
    three_ways(what, &mut patched, before, &mut dom);
}

/// [`parity`] for an edit that needs a union this fixture actually draws as
/// its recovered fallback pieces.
///
/// A fixture with no such union is *skipped*, loudly: `TestPlace.rbxl` — the
/// default — has no `UnionOperation` at all, and a hand-built stand-in would
/// compare the patch path against itself rather than against a real place's
/// baked CSG. Point `RBX_PARITY_FIXTURE` at a place that has one.
fn union_parity(what: &str, edit: impl Fn(&mut WeakDom, Ref)) {
    let path = fixture();
    let mut dom = rbx_viewer::read_place(&path).expect("the fixture parses");
    let mut patched = Headless::load(&path, true).expect("the fixture loads");
    frame(&mut patched);

    let Some(union) = fallback_union(&patched, &dom) else {
        println!(
            "{what}: skipped, {} draws no union as its recovered fallback pieces",
            path.display()
        );
        return;
    };
    println!(
        "{what}: on {union:?}, drawn as {} recovered pieces",
        patched.fallback_pieces(union)
    );

    let before = dom.clone();
    edit(&mut dom, union);
    three_ways(what, &mut patched, before, &mut dom);
}

/// The comparison both entry points share: the edit's own log applied to the
/// edited DOM, then to the one from before it, then to the edited one again.
fn three_ways(what: &str, patched: &mut Headless, before: WeakDom, dom: &mut WeakDom) {
    let log = dom.take_changes();
    assert!(!log.is_empty(), "{what} changed nothing");
    let after = dom.clone();

    check(patched, &after, &log, &format!("{what} (forward)"));
    check(patched, &before, &log, &format!("{what} (undo)"));
    check(patched, &after, &log, &format!("{what} (redo)"));
}

/// The first instance the viewer draws as several recovered pieces — which
/// depends on what that union's asset carved to, not on its class, so the
/// renderer is asked rather than the DOM.
fn fallback_union(headless: &Headless, dom: &WeakDom) -> Option<Ref> {
    let mut stack = dom.root_refs().to_vec();
    let mut found = None;
    while let Some(referent) = stack.pop() {
        if let Some(instance) = dom.get(referent) {
            stack.extend_from_slice(instance.children());
        }
        if headless.fallback_pieces(referent) > 0 {
            found = Some(referent);
        }
    }
    found
}

/// Every `BasePart` under `Workspace`, in walk order.
fn parts(dom: &WeakDom) -> Vec<Ref> {
    let workspace = dom
        .root_refs()
        .iter()
        .copied()
        .find(|referent| dom.get(*referent).is_some_and(|i| i.class() == "Workspace"))
        .expect("a Workspace");
    let mut stack = vec![workspace];
    let mut found = Vec::new();
    while let Some(referent) = stack.pop() {
        let Some(instance) = dom.get(referent) else {
            continue;
        };
        stack.extend_from_slice(instance.children());
        // A `size` as well as a `CFrame`: a `Camera` has the latter alone,
        // and moving it draws nothing to compare.
        let properties = instance.properties();
        if properties.contains_key("CFrame")
            && properties.contains_key("size")
            && instance.class() != "Terrain"
        {
            found.push(referent);
        }
    }
    found
}

fn workspace(dom: &WeakDom) -> Ref {
    dom.parent(parts(dom)[0]).expect("a part's parent")
}

/// The one instance called `name`, wherever it hangs.
fn named(dom: &WeakDom, name: &str) -> Ref {
    let mut stack = dom.root_refs().to_vec();
    while let Some(referent) = stack.pop() {
        let Some(instance) = dom.get(referent) else {
            continue;
        };
        if instance.name() == name {
            return referent;
        }
        stack.extend_from_slice(instance.children());
    }
    panic!("no instance named {name}");
}

fn udim2(scale: f32) -> Variant {
    let axis = UDim { scale, offset: 0 };
    Variant::UDim2(UDim2 { x: axis, y: axis })
}

fn nudge(dom: &mut WeakDom, referent: Ref, dy: f32) {
    let Some(Variant::CFrame(mut frame)) = dom
        .get(referent)
        .and_then(|instance| instance.properties().get("CFrame").cloned())
    else {
        panic!("a part with a CFrame");
    };
    frame.position.y += dy;
    dom.set_property(referent, "CFrame", Variant::CFrame(frame))
        .expect("the part exists");
}

#[test]
#[ignore = "needs a GPU"]
fn an_inserted_part_draws_as_a_rebuild_draws_it() {
    parity("insert", |dom| {
        let workspace = workspace(dom);
        let part = dom.new_instance("Part", "Inserted", Some(workspace));
        dom.set_property(
            part,
            "size",
            Variant::Vector3(rbx_dom::Vector3Data {
                x: 6.0,
                y: 3.0,
                z: 6.0,
            }),
        )
        .unwrap();
        dom.set_property(
            part,
            "CFrame",
            Variant::CFrame(rbx_dom::CFrameData {
                position: rbx_dom::Vector3Data {
                    x: 3.0,
                    y: 6.0,
                    z: 3.0,
                },
                rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            }),
        )
        .unwrap();
        dom.set_property(
            part,
            "Color3uint8",
            Variant::Color3uint8 {
                r: 200,
                g: 40,
                b: 40,
            },
        )
        .unwrap();
    });
}

#[test]
#[ignore = "needs a GPU"]
fn a_deleted_part_draws_as_a_rebuild_draws_it() {
    parity("delete", |dom| {
        let part = parts(dom)[0];
        dom.remove(part);
    });
}

#[test]
#[ignore = "needs a GPU"]
fn a_moved_part_draws_as_a_rebuild_draws_it() {
    parity("move", |dom| {
        let part = parts(dom)[0];
        nudge(dom, part, 5.0);
    });
}

#[test]
#[ignore = "needs a GPU"]
fn a_batch_of_moved_parts_draws_as_a_rebuild_draws_it() {
    parity("batch move", |dom| {
        for part in parts(dom).into_iter().take(100) {
            nudge(dom, part, 2.0);
        }
    });
}

#[test]
#[ignore = "needs a GPU"]
fn a_reparent_inside_workspace_draws_as_a_rebuild_draws_it() {
    parity("reparent inside Workspace", |dom| {
        let part = parts(dom)[0];
        let workspace = workspace(dom);
        let model = dom.new_instance("Model", "Group", Some(workspace));
        dom.set_parent(part, Some(model));
    });
}

#[test]
#[ignore = "needs a GPU"]
fn a_reparent_out_of_workspace_draws_as_a_rebuild_draws_it() {
    parity("reparent out of Workspace", |dom| {
        let part = parts(dom)[0];
        let staging = dom.new_instance("Folder", "Staging", None);
        dom.set_parent(part, Some(staging));
    });
}

#[test]
#[ignore = "needs a GPU"]
fn a_script_touching_several_instances_draws_as_a_rebuild_draws_it() {
    parity("multi-instance script", |dom| {
        let parts = parts(dom);
        for (index, part) in parts.iter().take(5).enumerate() {
            dom.set_property(*part, "Transparency", Variant::Float32(0.3))
                .unwrap();
            dom.set_property(
                *part,
                "Color3uint8",
                Variant::Color3uint8 {
                    r: 40 * index as u8,
                    g: 120,
                    b: 220,
                },
            )
            .unwrap();
            nudge(dom, *part, 1.0);
        }
        let workspace = workspace(dom);
        dom.set_name(workspace, "Renamed").unwrap();
    });
}

#[test]
#[ignore = "needs a GPU"]
fn a_moved_union_draws_as_a_rebuild_draws_it() {
    union_parity("union move", |dom, union| nudge(dom, union, 6.0));
}

// A union's own colour paints the mesh its boolean would have produced, and
// nothing at all when it is drawn as its pieces — which is exactly what makes
// this worth pinning: the patch has to agree with the rebuild about drawing
// *no* change, rather than repainting pieces a rebuild would leave alone.
#[test]
#[ignore = "needs a GPU"]
fn a_recoloured_union_draws_as_a_rebuild_draws_it() {
    union_parity("union recolour", |dom, union| {
        dom.set_property(
            union,
            "Color3uint8",
            Variant::Color3uint8 {
                r: 10,
                g: 220,
                b: 90,
            },
        )
        .expect("the union exists");
    });
}

#[test]
#[ignore = "needs a GPU"]
fn a_union_turned_transparent_draws_as_a_rebuild_draws_it() {
    union_parity("union transparency", |dom, union| {
        dom.set_property(union, "Transparency", Variant::Float32(1.0))
            .expect("the union exists");
    });
}

#[test]
#[ignore = "needs a GPU"]
fn a_deleted_union_draws_as_a_rebuild_draws_it() {
    union_parity("union delete", |dom, union| {
        dom.remove(union);
    });
}

// A GUI tree is planned from its container down, so a `Frame` dragged from
// a `ScreenGui` onto a part's `BillboardGui` leaves two plans stale, not
// one: the overlay's, which kept drawing the frame where it used to be, as
// well as the canvas's.
#[test]
#[ignore = "needs a GPU"]
fn a_frame_moved_from_a_screen_gui_to_a_billboard_gui_draws_as_a_rebuild_draws_it() {
    staged(
        "reparent a Frame between GUI containers",
        |dom| {
            let workspace = workspace(dom);
            let part = parts(dom)[0];
            let screen = dom.new_instance("ScreenGui", "Screen", Some(workspace));
            let frame = dom.new_instance("Frame", "Moved", Some(screen));
            for (name, value) in [
                ("Size", udim2(0.3)),
                ("Position", udim2(0.1)),
                (
                    "BackgroundColor3",
                    Variant::Color3(Color3Data {
                        r: 1.0,
                        g: 0.0,
                        b: 0.0,
                    }),
                ),
            ] {
                dom.set_property(frame, name, value).unwrap();
            }
            let billboard = dom.new_instance("BillboardGui", "Board", Some(part));
            // Scale is studs on a `BillboardGui`.
            dom.set_property(billboard, "Size", udim2(6.0)).unwrap();
        },
        |dom| {
            let frame = named(dom, "Moved");
            let billboard = named(dom, "Board");
            dom.set_parent(frame, Some(billboard));
        },
    );
}

// A class change re-derives the instance whole — a wedge is not a block —
// and is the one edit that can move a live instance out of the pass that
// draws it: a part turned into a `Folder` has to stop being drawn.
#[test]
#[ignore = "needs a GPU"]
fn a_part_given_another_class_draws_as_a_rebuild_draws_it() {
    for class in ["WedgePart", "CornerWedgePart", "MeshPart", "Folder"] {
        parity(&format!("Part to {class}"), |dom| {
            let part = parts(dom)[0];
            dom.set_class(part, class).unwrap();
        });
    }
}

#[test]
#[ignore = "needs a GPU"]
fn a_light_given_another_class_draws_as_a_rebuild_draws_it() {
    for class in ["SpotLight", "SurfaceLight", "StringValue"] {
        staged(
            &format!("PointLight to {class}"),
            |dom| {
                let part = parts(dom)[0];
                let light = dom.new_instance("PointLight", "Lamp", Some(part));
                for (name, value) in [
                    ("Brightness", Variant::Float32(8.0)),
                    ("Range", Variant::Float32(40.0)),
                    (
                        "Color",
                        Variant::Color3(Color3Data {
                            r: 1.0,
                            g: 0.2,
                            b: 0.2,
                        }),
                    ),
                ] {
                    dom.set_property(light, name, value).unwrap();
                }
            },
            |dom| {
                let light = named(dom, "Lamp");
                dom.set_class(light, class).unwrap();
            },
        );
    }
}

#[test]
#[ignore = "needs a GPU"]
fn a_frame_given_another_class_draws_as_a_rebuild_draws_it() {
    for class in ["TextLabel", "ImageLabel", "Folder"] {
        staged(
            &format!("Frame to {class}"),
            |dom| {
                let starter = named(dom, "StarterGui");
                let screen = dom.new_instance("ScreenGui", "Screen", Some(starter));
                let frame = dom.new_instance("Frame", "Panel", Some(screen));
                for (name, value) in [
                    ("Size", udim2(0.4)),
                    ("Position", udim2(0.1)),
                    (
                        "BackgroundColor3",
                        Variant::Color3(Color3Data {
                            r: 0.1,
                            g: 0.3,
                            b: 1.0,
                        }),
                    ),
                ] {
                    dom.set_property(frame, name, value).unwrap();
                }
            },
            |dom| {
                let frame = named(dom, "Panel");
                dom.set_class(frame, class).unwrap();
            },
        );
    }
}

/// The edits that cannot be patched take the whole scene through
/// `Headless::reload` instead, and the render thread has to come out of that
/// still drawing: a viewport that goes black (or stops handing frames back)
/// after a scripted edit is the failure this guards, and it is one no
/// parity check above would catch — a rebuild compared against a rebuild
/// agrees with itself perfectly while both draw nothing.
#[test]
#[ignore = "needs a GPU"]
fn a_rebuild_leaves_the_renderer_drawing() {
    let path = fixture();
    let mut dom = rbx_viewer::read_place(&path).expect("the fixture parses");
    let mut headless = Headless::load(&path, true).expect("the fixture loads");
    assert!(
        !blank(&frame(&mut headless)),
        "the fixture draws something to begin with"
    );

    // A `Sky` is the cheapest edit `apply_changes` refuses to patch.
    // Removing the place's own is the one whose effect is unmistakable:
    // the frame falls back to this renderer's procedural sky. A place with
    // no `Sky` at all gets one instead, which rebuilds just the same but
    // may legitimately draw the same pixels (an empty `Sky` is the default
    // sky), so the picture is only compared in the first case.
    let existing = sky(&dom);
    match existing {
        Some(referent) => {
            dom.remove(referent);
        }
        None => {
            let lighting = named(&dom, "Lighting");
            dom.new_instance("Sky", "Sky", Some(lighting));
        }
    }
    let log = dom.take_changes();
    assert_eq!(
        headless
            .apply_changes(&dom, &log)
            .expect("the edit applies"),
        Applied::Rebuilt(rbx_viewer::Rebuild::Sky),
    );

    let after = frame(&mut headless);
    assert!(!blank(&after), "the frame after a rebuild is blank");
    // And it is the picture a scene rebuilt from the same DOM draws — the
    // same bar every patched edit above is held to, applied to the path
    // that gives up on patching.
    let reference = rebuilt(&dom);
    let ae = differing(&after, &reference);
    assert_eq!(
        ae, 0,
        "a rebuild in place: {ae} pixels differ from a reload"
    );

    // Still drawing several frames later, which is where a renderer left
    // holding a stale target would show it.
    for _ in 0..3 {
        assert!(!blank(&frame(&mut headless)), "a later frame is blank");
    }
}

/// The place's own `Sky`, if it has one.
fn sky(dom: &WeakDom) -> Option<Ref> {
    let mut pending: Vec<Ref> = dom.root_refs().to_vec();
    while let Some(referent) = pending.pop() {
        let instance = dom.get(referent)?;
        if instance.class() == "Sky" {
            return Some(referent);
        }
        pending.extend(instance.children());
    }
    None
}

/// Whether every pixel is the same colour — a frame with nothing drawn in
/// it, whatever that colour turned out to be.
fn blank(pixels: &[u8]) -> bool {
    let mut chunks = pixels.chunks(4);
    let first = chunks.next().unwrap_or(&[0; 4]);
    chunks.all(|pixel| pixel == first)
}

/// Selecting a part has to show in the frame: the cue is the silhouette of
/// the geometry it covers (see `renderer::cue`), so a selected part draws
/// differently from an unselected one — and goes back to the unselected
/// picture when the selection is cleared.
#[test]
#[ignore = "needs a GPU"]
fn a_selected_part_is_cued_in_the_frame() {
    let path = fixture();
    let dom = rbx_viewer::read_place(&path).expect("the fixture parses");
    let mut headless = Headless::load(&path, true).expect("the fixture loads");
    let bare = frame(&mut headless);

    let part = *parts(&dom).first().expect("the fixture has a part");
    headless.set_selection(&[rbx_viewer::pick::Selected::read(
        &dom,
        &rbx_reflection::ReflectionDatabase::embedded(),
        part,
    )]);
    let cued = frame(&mut headless);
    assert!(
        differing(&bare, &cued) > 0,
        "a selected part drew exactly the unselected frame: no cue at all"
    );

    headless.set_selection(&[]);
    let cleared = frame(&mut headless);
    assert_eq!(
        differing(&bare, &cleared),
        0,
        "clearing the selection left the cue behind"
    );
}

/// The same, at every graphics level — the cue's mask follows the scene
/// pass's own sample count, and a multisampled one is a different texture
/// type in the shader (see `renderer::highlight::pipelines`). A level that
/// silently dropped the cue would leave the editor with nothing to show for
/// a selection, which is exactly what its users would report as "the
/// outline is gone".
#[test]
#[ignore = "needs a GPU"]
fn a_selected_part_is_cued_at_every_graphics_level() {
    let path = fixture();
    let dom = rbx_viewer::read_place(&path).expect("the fixture parses");
    let part = *parts(&dom).first().expect("the fixture has a part");
    let selected = rbx_viewer::pick::Selected::read(
        &dom,
        &rbx_reflection::ReflectionDatabase::embedded(),
        part,
    );

    for level in [1u8, 8, 14, 21] {
        let mut headless = Headless::load(&path, true).expect("the fixture loads");
        headless.set_quality(rbx_viewer::QualityLevel::Level(level));
        let bare = frame(&mut headless);
        headless.set_selection(std::slice::from_ref(&selected));
        let cued = frame(&mut headless);
        assert!(
            differing(&bare, &cued) > 0,
            "level {level}: a selected part drew exactly the unselected frame"
        );
    }
}

/// A selected part moved by an edit — a viewport drag, a typed `CFrame`, an
/// undo, which all reach the viewer as this same patched log — carries its
/// selection cue with it: the frame is the one a rebuild of the moved DOM,
/// with the same selection, draws.
#[test]
#[ignore = "needs a GPU"]
fn a_moved_selected_part_takes_its_cue_with_it() {
    let path = fixture();
    let mut dom = rbx_viewer::read_place(&path).expect("the fixture parses");
    let part = *parts(&dom).first().expect("the fixture has a part");
    let selected = [rbx_viewer::pick::Selected::read(
        &dom,
        &rbx_reflection::ReflectionDatabase::embedded(),
        part,
    )];
    let selected_frame = |dom: &WeakDom| {
        let mut reference = Headless::load(&path, true).expect("the fixture loads");
        reference.reload(dom).expect("the DOM rebuilds");
        reference.set_selection(&selected);
        frame(&mut reference)
    };

    let mut patched = Headless::load(&path, true).expect("the fixture loads");
    patched.set_selection(&selected);
    frame(&mut patched);
    let before = dom.clone();
    nudge(&mut dom, part, 5.0);
    let log = dom.take_changes();

    for (what, state) in [("forward", &dom), ("undo", &before)] {
        let applied = patched
            .apply_changes(state, &log)
            .expect("the edit applies");
        assert_eq!(applied, Applied::Patched, "{what} must be patched");
        let ae = differing(&frame(&mut patched), &selected_frame(state));
        assert_eq!(ae, 0, "{what}: {ae} pixels differ from a rebuild");
    }
}
