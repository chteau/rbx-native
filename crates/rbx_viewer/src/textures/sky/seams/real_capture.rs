//! Re-derives [`super::ADJACENT_BORDERS`] from the pixels of a real six-image
//! sky, which is where that table came from in the first place.
//!
//! Ignored by default: it reads six sky panels out of the on-disk asset cache.
//! Run it with:
//!
//! ```text
//! cargo test -p rbx_viewer --lib sky::seams -- --ignored --nocapture
//! ```

use rbx_assets::{decode_image, sniff, Asset, AssetCache};

use super::super::SkyFace;
use super::{Edge, ADJACENT_BORDERS, EDGES};

/// Six distinct cloud panels, which make the seams measurable at all — a
/// uniform gradient would hide any mistake.
const PANELS: [(SkyFace, u64); 6] = [
    (SkyFace::Rt, 14556579194),
    (SkyFace::Lf, 14556567190),
    (SkyFace::Up, 14556582671),
    (SkyFace::Dn, 14556564331),
    (SkyFace::Bk, 14556558767),
    (SkyFace::Ft, 14556576655),
];

/// A border pixel line agrees with its partner this closely, per channel out of
/// 255. Measured worst case is 1.13 (JPEG-ish ringing on a resaved panel).
const AGREES: f64 = 2.0;

/// Every other pairing of that border is at least this far off. Measured best
/// rival is 22.3, so the gap between a real seam and a wrong one is an order of
/// magnitude, not a judgement call.
const DIFFERS: f64 = 10.0;

struct Panel {
    face: SkyFace,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Panel {
    fn rgb(&self, x: u32, y: u32) -> [i32; 3] {
        let at = ((y * self.width + x) * 4) as usize;
        [
            i32::from(self.pixels[at]),
            i32::from(self.pixels[at + 1]),
            i32::from(self.pixels[at + 2]),
        ]
    }

    /// One border line, walked the way [`Edge`] defines.
    fn border(&self, edge: Edge) -> Vec<[i32; 3]> {
        match edge {
            Edge::Top => (0..self.width).map(|x| self.rgb(x, 0)).collect(),
            Edge::Bottom => (0..self.width)
                .map(|x| self.rgb(x, self.height - 1))
                .collect(),
            Edge::Left => (0..self.height).map(|y| self.rgb(0, y)).collect(),
            Edge::Right => (0..self.height)
                .map(|y| self.rgb(self.width - 1, y))
                .collect(),
        }
    }
}

fn load() -> Vec<Panel> {
    let cache = AssetCache::new(None).expect("an asset cache directory");

    PANELS
        .iter()
        .map(|&(face, id)| {
            let bytes = cache.get_id(id).unwrap_or_else(|| {
                panic!("asset {id} ({face:?}) is not cached; render a sky capture first")
            });
            let kind = sniff(&bytes);
            let image = decode_image(&Asset { bytes, kind }).expect("a decodable panel");
            let (width, height) = image.dimensions();

            Panel {
                face,
                width,
                height,
                pixels: image.into_raw(),
            }
        })
        .collect()
}

fn panel(panels: &[Panel], face: SkyFace) -> &Panel {
    panels
        .iter()
        .find(|panel| panel.face == face)
        .expect("all six panels are loaded")
}

/// Mean absolute per-channel difference between two border lines, `b` optionally
/// walked backwards.
fn mismatch(a: &[[i32; 3]], b: &[[i32; 3]], reversed: bool) -> f64 {
    assert_eq!(a.len(), b.len(), "panels of different sizes");

    let total: i64 = a
        .iter()
        .enumerate()
        .map(|(at, left)| {
            let right = if reversed { b[b.len() - 1 - at] } else { b[at] };
            (0..3)
                .map(|c| i64::from((left[c] - right[c]).abs()))
                .sum::<i64>()
        })
        .sum();

    total as f64 / (a.len() * 3) as f64
}

#[test]
#[ignore = "needs sky panels in the local asset cache"]
fn the_real_capture_borders_agree_only_as_this_table_says() {
    let panels = load();

    for &(a_face, a_edge, b_face, b_edge, reversed) in &ADJACENT_BORDERS {
        let line = panel(&panels, a_face).border(a_edge);
        let partner = panel(&panels, b_face).border(b_edge);

        let agreement = mismatch(&line, &partner, reversed);
        println!(
            "{a_face:?}{a_edge:?} <-> {b_face:?}{b_edge:?} {} {agreement:6.2}",
            if reversed { "reversed" } else { "forward " }
        );
        assert!(
            agreement < AGREES,
            "{a_face:?}{a_edge:?} should join {b_face:?}{b_edge:?} but is off by {agreement:.2}"
        );

        // And no rival pairing of that same border comes anywhere close, which is
        // what makes the assembly the only one the pixels allow.
        for other in &panels {
            for edge in EDGES {
                for walk in [false, true] {
                    if other.face == a_face
                        || (other.face == b_face && edge == b_edge && walk == reversed)
                    {
                        continue;
                    }
                    let rival = mismatch(&line, &other.border(edge), walk);
                    assert!(
                        rival > DIFFERS,
                        "{a_face:?}{a_edge:?} also fits {:?}{edge:?} (reversed {walk}) at {rival:.2}",
                        other.face
                    );
                }
            }
        }
    }
}

// Which pole is the zenith is the one thing the pixels cannot settle: it comes
// from the property names alone. Brightness is no help and must not be turned
// into a check — this sky is painted from above a sea of clouds, so its `Dn`
// panel (mean 184/255) is *brighter* than its `Up` panel (157/255), which holds
// the moon. Both pole panels do show the radial singularity a pole has, and the
// side panels are stored upright with their horizon across the middle.
