//! A skinned `MeshPart` bends with the parts its bones are named after: a
//! ribbon skinned half to `UpperArm` (the mesh part itself) and half to
//! `LowerArm` (an invisible sibling part in the same `Model`) swings its lower
//! half out sideways when `LowerArm` is turned a quarter about the joint — as
//! a patch through `Headless::apply_changes`, not a reload.
//!
//! The mesh is written here as `version 4.00` bytes and served from the local
//! asset cache as `rbxasset://rbx-native-local/...` native content, the way
//! the auto-rigger previews its meshes.
//!
//! Needs a GPU. Without one the test says so and passes, rather than being
//! `#[ignore]`d: a software rasteriser (Mesa's lavapipe,
//! `VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json`) is enough.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use rbx_dom::{CFrameData, Instance, Ref, Variant, Vector3Data, WeakDom};
use rbx_viewer::{Applied, Headless};

const SIZE: (u32, u32) = (160, 120);
const DRAIN: usize = 8;
const IDENTITY: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
/// 90° about +Z, row-major: +X goes to +Y.
const QUARTER_Z: [f32; 9] = [0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
const NATIVE_PATH: &str = "rbx-native-local/tests/rbx_viewer-skinned-arm.mesh";

const WORKSPACE: u32 = 1;
const ARM: u32 = 2;
const UPPER: u32 = 3;
const LOWER: u32 = 4;

/// Rows of the ribbon, top to bottom; the joint is the middle one.
const ROWS: [f32; 5] = [3.0, 1.5, 0.0, -1.5, -3.0];

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/tests/TestPlace.rbxl")
}

/// A minimal `version 4.00` writer: one subset over every vertex, no LODs,
/// `bones` as `(name, parent, position)` with identity rotation, and each
/// vertex's envelope as subset slots and weight bytes.
struct V4 {
    vertices: Vec<([f32; 3], [f32; 3])>,
    envelopes: Vec<([u8; 4], [u8; 4])>,
    faces: Vec<[u32; 3]>,
    bones: Vec<(&'static str, Option<u16>, [f32; 3])>,
}

impl V4 {
    fn bytes(&self) -> Vec<u8> {
        let mut names = Vec::new();
        let mut name_offsets = Vec::new();
        for (name, ..) in &self.bones {
            name_offsets.push(names.len() as u32);
            names.extend_from_slice(name.as_bytes());
            names.push(0);
        }

        let mut out = b"version 4.00\n".to_vec();
        out.extend(24u16.to_le_bytes());
        out.extend(0u16.to_le_bytes()); // lod_type
        out.extend((self.vertices.len() as u32).to_le_bytes());
        out.extend((self.faces.len() as u32).to_le_bytes());
        out.extend(0u16.to_le_bytes()); // num_lods
        out.extend((self.bones.len() as u16).to_le_bytes());
        out.extend((names.len() as u32).to_le_bytes());
        out.extend(1u16.to_le_bytes()); // num_subsets
        out.push(0); // num_high_quality_lods
        out.push(0); // unused

        for (position, normal) in &self.vertices {
            for value in position.iter().chain(normal).chain(&[0.0, 0.0]) {
                out.extend(value.to_le_bytes());
            }
            out.extend([0, 0, 0, 127]); // tangent
            out.extend([255; 4]); // color
        }
        for (slots, weights) in &self.envelopes {
            out.extend(slots);
            out.extend(weights);
        }
        for face in &self.faces {
            for index in face {
                out.extend(index.to_le_bytes());
            }
        }
        for ((_, parent, position), offset) in self.bones.iter().zip(name_offsets) {
            out.extend(offset.to_le_bytes());
            let parent = parent.unwrap_or(0xFFFF);
            out.extend(parent.to_le_bytes());
            out.extend(parent.to_le_bytes());
            out.extend(0f32.to_le_bytes()); // culling
            for value in IDENTITY.iter().chain(position) {
                out.extend(value.to_le_bytes());
            }
        }
        out.extend(&names);
        for word in [
            0,
            self.faces.len() as u32,
            0,
            self.vertices.len() as u32,
            self.bones.len() as u32,
        ] {
            out.extend(word.to_le_bytes());
        }
        for slot in 0..26u16 {
            let bone = if (slot as usize) < self.bones.len() {
                slot
            } else {
                0xFFFF
            };
            out.extend(bone.to_le_bytes());
        }
        out
    }
}

/// A ribbon one stud wide and six long down native Y, facing −Z, its joint
/// at the origin: the top half weighted to `UpperArm`, the bottom half to
/// `LowerArm`, the joint row split between them.
fn ribbon() -> Vec<u8> {
    let mut mesh = V4 {
        vertices: Vec::new(),
        envelopes: Vec::new(),
        faces: Vec::new(),
        bones: vec![
            ("UpperArm", None, [0.0, 1.5, 0.0]),
            ("LowerArm", Some(0), [0.0, 0.0, 0.0]),
        ],
    };
    for (row, &y) in ROWS.iter().enumerate() {
        for x in [-0.5, 0.5] {
            mesh.vertices.push(([x, y, 0.0], [0.0, 0.0, -1.0]));
            let weights = match y {
                y if y > 0.0 => [255, 0, 0, 0],
                0.0 => [128, 127, 0, 0],
                _ => [0, 255, 0, 0],
            };
            mesh.envelopes.push(([0, 1, 0, 0], weights));
        }
        if row + 1 < ROWS.len() {
            let top = row as u32 * 2;
            mesh.faces.push([top, top + 1, top + 2]);
            mesh.faces.push([top + 1, top + 3, top + 2]);
        }
    }
    mesh.bytes()
}

fn cframe(position: [f32; 3], rotation: [f32; 9]) -> Variant {
    let [x, y, z] = position;
    Variant::CFrame(CFrameData {
        position: Vector3Data { x, y, z },
        rotation,
    })
}

fn vector3([x, y, z]: [f32; 3]) -> Variant {
    Variant::Vector3(Vector3Data { x, y, z })
}

/// `Workspace > Arm (Model) > {UpperArm (green MeshPart), LowerArm
/// (invisible Part)}`.
fn place() -> WeakDom {
    let mut dom = WeakDom::new();
    dom.insert(Instance::new(Ref::new(WORKSPACE), "Workspace", "Workspace"));
    dom.insert(Instance::new(Ref::new(ARM), "Model", "Arm"));

    let mut upper = Instance::new(Ref::new(UPPER), "MeshPart", "UpperArm");
    let properties = upper.properties_mut();
    properties.insert(
        "MeshId".to_string(),
        Variant::String(format!("rbxasset://{NATIVE_PATH}")),
    );
    properties.insert("size".to_string(), vector3([1.0, 6.0, 0.2]));
    properties.insert("InitialSize".to_string(), vector3([1.0, 6.0, 0.2]));
    properties.insert("CFrame".to_string(), cframe([0.0; 3], IDENTITY));
    properties.insert(
        "Color3uint8".to_string(),
        Variant::Color3uint8 { r: 0, g: 255, b: 0 },
    );
    dom.insert(upper);

    let mut lower = Instance::new(Ref::new(LOWER), "Part", "LowerArm");
    let properties = lower.properties_mut();
    properties.insert("size".to_string(), vector3([1.0, 3.0, 0.2]));
    properties.insert("CFrame".to_string(), cframe([0.0, -1.5, 0.0], IDENTITY));
    properties.insert("Transparency".to_string(), Variant::Float32(1.0));
    dom.insert(lower);

    dom.set_parent(Ref::new(WORKSPACE), None);
    dom.set_parent(Ref::new(ARM), Some(Ref::new(WORKSPACE)));
    dom.set_parent(Ref::new(UPPER), Some(Ref::new(ARM)));
    dom.set_parent(Ref::new(LOWER), Some(Ref::new(ARM)));
    dom.take_changes();
    dom
}

fn settle(headless: &mut Headless) {
    let until = Instant::now() + Duration::from_secs(20);
    while Instant::now() < until {
        let swapped = headless.swap_assets();
        if headless.assets_in_flight() == 0 && !swapped {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("the mesh never landed");
}

fn frame(headless: &mut Headless) -> Vec<u8> {
    for _ in 0..DRAIN {
        headless.render_frame(SIZE.0, SIZE.1).expect("a frame");
    }
    headless.take_frame().expect("the last frame");
    headless.render_frame(SIZE.0, SIZE.1).expect("a frame");
    headless
        .take_frame()
        .expect("the compared frame")
        .expect("one frame in flight")
        .pixels
}

/// The green mesh, however it is lit.
fn green(pixel: &[u8]) -> bool {
    let [r, g, b] = [pixel[0], pixel[1], pixel[2]].map(u16::from);
    g > 30 && g > 2 * r + 8 && g > 2 * b + 8
}

/// Green pixels whose row and column, as fractions of the frame, pass
/// `inside`.
fn count(pixels: &[u8], inside: impl Fn(f32, f32) -> bool) -> usize {
    let (width, height) = (SIZE.0 as usize, SIZE.1 as usize);
    (0..height)
        .flat_map(|row| (0..width).map(move |column| (row, column)))
        .filter(|&(row, column)| {
            let at = (row * width + column) * 4;
            green(&pixels[at..at + 4])
                && inside(
                    (row as f32 + 0.5) / height as f32,
                    (column as f32 + 0.5) / width as f32,
                )
        })
        .count()
}

/// A band of rows where only the hanging lower half is: world y from about
/// −1 to −2.5 seen from nine studs off, whatever the field of view's axis.
fn below_the_joint(row: f32, _column: f32) -> bool {
    (0.61..0.69).contains(&row)
}

/// Columns well clear of the hanging ribbon's width, on either side.
fn out_to_the_side(_row: f32, column: f32) -> bool {
    (column - 0.5).abs() > 0.125
}

#[test]
fn turning_the_lower_arm_bends_the_skinned_mesh_as_a_patch() {
    let cache = rbx_assets::AssetCache::new(None).expect("the asset cache opens");
    cache
        .put_native(NATIVE_PATH, &ribbon())
        .expect("the mesh is cached");

    let mut headless = match Headless::load(&fixture(), false) {
        Ok(headless) => headless,
        Err(why) => {
            println!("skipped: no GPU to render with ({why})");
            return;
        }
    };
    let mut dom = place();
    headless.reload(&dom).expect("the DOM rebuilds");
    settle(&mut headless);
    headless.open_at([0.0, 0.0, -9.0], [0.0, 0.0, 0.0], 70.0);

    let rest = frame(&mut headless);
    assert!(count(&rest, |_, _| true) > 200, "the ribbon is drawn");
    assert!(
        count(&rest, below_the_joint) > 20,
        "at rest the lower half hangs below the joint"
    );
    assert_eq!(count(&rest, out_to_the_side), 0, "and nothing sticks out");

    // A quarter turn about +Z around the joint at the origin: the lower
    // arm's centre swings from (0, −1.5) to (1.5, 0).
    dom.set_property(
        Ref::new(LOWER),
        "CFrame",
        cframe([1.5, 0.0, 0.0], QUARTER_Z),
    )
    .expect("a part takes a CFrame");
    let changes = dom.take_changes();
    assert_eq!(
        headless.apply_changes(&dom, &changes).expect("applies"),
        Applied::Patched,
        "a pose is a patch, not a reload"
    );
    let bent = frame(&mut headless);
    assert_eq!(
        count(&bent, below_the_joint),
        0,
        "the lower half no longer hangs below the joint"
    );
    assert!(
        count(&bent, out_to_the_side) > 20,
        "it sticks out to the side instead"
    );
    assert!(
        count(&bent, |row, _| row < 0.45) > 20,
        "the upper half is still drawn above the joint"
    );

    // And back: the rest frame exactly.
    dom.set_property(
        Ref::new(LOWER),
        "CFrame",
        cframe([0.0, -1.5, 0.0], IDENTITY),
    )
    .expect("a part takes a CFrame");
    let changes = dom.take_changes();
    assert_eq!(
        headless.apply_changes(&dom, &changes).expect("applies"),
        Applied::Patched
    );
    assert_eq!(frame(&mut headless), rest, "the rest pose draws as before");
}
