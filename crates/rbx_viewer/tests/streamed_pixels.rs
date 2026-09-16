//! Proves the picture a streamed asset lands into is the picture the place
//! would have had if the asset had been there all along.
//!
//! Needs a GPU and a real place file with two distinct `MeshId`s among its
//! `MeshPart`s, so it is `#[ignore]`d and opt-in:
//!
//! ```text
//! RBX_STREAMING_FIXTURE=../rbx-native-fixtures/places/marked.rbxl \
//!   cargo test -p rbx_viewer --test streamed_pixels -- --ignored --nocapture
//! ```
//!
//! No network: the fixture's assets must already be in the on-disk cache (the
//! same requirement `BENCHMARKS.md` states), and "never seen" is staged with
//! `Headless::forget_asset` rather than by evicting that cache.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use rbx_dom::{Change, Ref, Variant, WeakDom};
use rbx_viewer::{Applied, Headless};

const SIZE: (u32, u32) = (640, 360);
/// Frames drawn before a comparison, so `Renderer::draw`'s per-frame texture
/// upload budget has nothing left queued: two pictures are only comparable
/// once both have every texture they are meant to have.
const DRAIN: usize = 64;
const LIMIT: Duration = Duration::from_secs(60);

fn fixture() -> PathBuf {
    PathBuf::from(
        std::env::var("RBX_STREAMING_FIXTURE")
            .expect("set RBX_STREAMING_FIXTURE to a real .rbxl place file"),
    )
}

/// Drives the loader until every asset it asked for has landed and been folded
/// in, then drains the upload backlog and hands back a finished frame.
fn settled_frame(headless: &mut Headless) -> Vec<u8> {
    let waited = Instant::now();
    loop {
        let swapped = headless.swap_assets();
        if headless.assets_in_flight() == 0 && !swapped {
            break;
        }
        assert!(
            waited.elapsed() < LIMIT,
            "the place never finished loading its assets"
        );
    }
    drained_frame(headless)
}

fn drained_frame(headless: &mut Headless) -> Vec<u8> {
    for _ in 0..DRAIN {
        headless
            .render_frame(SIZE.0, SIZE.1)
            .expect("a frame should render");
    }
    headless
        .take_frame()
        .expect("a frame should render")
        .expect("a frame was queued")
        .pixels
}

/// The first `MeshPart` in `Workspace` with a `MeshId`, and a second, different
/// `MeshId` found elsewhere in the place.
fn two_meshes(dom: &WeakDom) -> Option<(Ref, String, String)> {
    let mut found: Vec<(Ref, String)> = Vec::new();
    let mut stack: Vec<Ref> = dom
        .root_refs()
        .iter()
        .copied()
        .filter(|referent| {
            dom.get(*referent)
                .is_some_and(|instance| instance.class() == "Workspace")
        })
        .collect();
    while let Some(referent) = stack.pop() {
        let Some(instance) = dom.get(referent) else {
            continue;
        };
        stack.extend_from_slice(instance.children());
        if instance.class() != "MeshPart" {
            continue;
        }
        if let Some(Variant::String(value)) = instance.properties().get("MeshId") {
            if !value.is_empty() {
                found.push((referent, value.clone()));
            }
        }
    }

    let (referent, resident) = found.first().cloned()?;
    let other = found
        .iter()
        .map(|(_, value)| value)
        .find(|value| **value != resident)?
        .clone();
    Some((referent, resident, other))
}

fn set_mesh(dom: &mut WeakDom, referent: Ref, value: &str) {
    dom.set_property(referent, "MeshId", Variant::String(value.to_string()))
        .expect("the fixture part should still exist");
}

/// The bar this whole change is judged against: the frame after an edit's
/// asset lands must be byte-for-byte the frame the place draws when that asset
/// was resident from the start.
#[test]
#[ignore = "needs a GPU and RBX_STREAMING_FIXTURE"]
fn a_landed_mesh_draws_what_a_resident_one_draws() {
    let path = fixture();
    let mut dom = rbx_viewer::read_place(&path).expect("fixture should parse");
    let Some((referent, resident, other)) = two_meshes(&dom) else {
        panic!("RBX_STREAMING_FIXTURE needs two MeshParts with different MeshIds");
    };

    // The reference: the place built with the edit already applied and every
    // asset it names decoded.
    let expected = {
        let mut headless = Headless::load(&path, true).expect("load");
        settled_frame(&mut headless);
        set_mesh(&mut dom, referent, &other);
        headless.reload(&dom).expect("reload");
        settled_frame(&mut headless)
    };

    // The path under test: the asset is unknown when the edit names it, the
    // edit draws the fallback, and the mesh arrives afterwards.
    let streamed = {
        set_mesh(&mut dom, referent, &resident);
        let mut headless = Headless::load(&path, true).expect("load");
        settled_frame(&mut headless);

        headless.forget_asset(&other);
        headless.reload(&dom).expect("reload without the asset");
        settled_frame(&mut headless);

        set_mesh(&mut dom, referent, &other);
        let change = [Change::Property {
            referent,
            name: "MeshId".to_string(),
        }];
        assert_eq!(
            headless
                .apply_changes(&dom, &change)
                .expect("patch should not fail"),
            Applied::Patched,
            "an edit naming an unseen mesh must be patched, not reloaded"
        );
        // Keeps the comparison below honest: if the fallback already looked
        // like the finished picture, matching it afterwards would prove
        // nothing about the asset having been swapped in at all.
        let fallback = drained_frame(&mut headless);
        assert_ne!(
            fallback, expected,
            "the fallback frame must not already be the finished one"
        );
        settled_frame(&mut headless)
    };

    let differing = expected
        .iter()
        .zip(&streamed)
        .filter(|(left, right)| left != right)
        .count();
    assert_eq!(
        differing,
        0,
        "{differing} of {} bytes differ between the streamed frame and the resident one",
        expected.len()
    );
}

/// The fallback itself: before the asset lands, the edited part draws the same
/// box a `MeshPart` whose mesh never resolved draws — which is what the viewer
/// showed for this case before any of it streamed.
#[test]
#[ignore = "needs a GPU and RBX_STREAMING_FIXTURE"]
fn the_frame_before_it_lands_is_the_fallback_box() {
    let path = fixture();
    let mut dom = rbx_viewer::read_place(&path).expect("fixture should parse");
    let Some((referent, resident, other)) = two_meshes(&dom) else {
        panic!("RBX_STREAMING_FIXTURE needs two MeshParts with different MeshIds");
    };

    // What a place whose mesh never resolves draws for this part: the edit is
    // applied and the asset is withheld from the reload that follows.
    let expected = {
        let mut headless = Headless::load(&path, true).expect("load");
        settled_frame(&mut headless);
        set_mesh(&mut dom, referent, &other);
        headless.forget_asset(&other);
        headless.reload(&dom).expect("reload without the asset");
        drained_frame(&mut headless)
    };

    // The same part reached by editing into the unseen asset instead.
    let patched = {
        set_mesh(&mut dom, referent, &resident);
        let mut headless = Headless::load(&path, true).expect("load");
        settled_frame(&mut headless);

        headless.forget_asset(&other);
        headless.reload(&dom).expect("reload without the asset");
        settled_frame(&mut headless);

        set_mesh(&mut dom, referent, &other);
        let change = [Change::Property {
            referent,
            name: "MeshId".to_string(),
        }];
        assert_eq!(
            headless
                .apply_changes(&dom, &change)
                .expect("patch should not fail"),
            Applied::Patched
        );
        drained_frame(&mut headless)
    };

    let differing = expected
        .iter()
        .zip(&patched)
        .filter(|(left, right)| left != right)
        .count();
    assert_eq!(
        differing,
        0,
        "{differing} of {} bytes differ between the patched fallback and a built one",
        expected.len()
    );
}
