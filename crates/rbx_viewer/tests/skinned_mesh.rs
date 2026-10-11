//! A skinned `MeshPart` bends with the parts its bones are named after: a
//! ribbon skinned half to `UpperArm` (the mesh part itself) and half to
//! `LowerArm` (an invisible sibling part in the same `Model`) swings its lower
//! half out sideways when `LowerArm` is turned a quarter about the joint — as
//! a patch through `Headless::apply_changes`, not a reload.
//!
//! The mesh is written here as `version 4.00` bytes and served from the local
//! asset cache as `rbxasset://rbx-native-local/...` native content, the way
//! the auto-rigger previews its meshes — including a rig inserted after the
//! place opened and swapped for a new one, the way the auto-rigger edits.
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
/// `Enum.Material.SmoothPlastic`: no maps to fetch, so nothing waits on the
/// network, and the one material layer every part here shares — a part
/// inserted on a layer the place never loaded is a reload, not a patch.
const SMOOTH_PLASTIC: u32 = 272;
const NATIVE_PATH: &str = "rbx-native-local/tests/rbx_viewer-skinned-arm.mesh";

const WORKSPACE: u32 = 1;
/// The opened place's arm: the model, then the upper and lower arm.
const ARM: u32 = 2;

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

/// Inserts `Arm (Model) > {UpperArm (green MeshPart), LowerArm (invisible
/// Part)}` under `Workspace` at referents from `base`, at rest, answering
/// with the model and the lower arm.
fn insert_arm(dom: &mut WeakDom, base: u32) -> (Ref, Ref) {
    let (model, upper_ref, lower_ref) = (Ref::new(base), Ref::new(base + 1), Ref::new(base + 2));
    dom.insert(Instance::new(model, "Model", "Arm"));

    let mut upper = Instance::new(upper_ref, "MeshPart", "UpperArm");
    let properties = upper.properties_mut();
    properties.insert(
        "MeshId".to_string(),
        Variant::String(format!("rbxasset://{NATIVE_PATH}")),
    );
    properties.insert("size".to_string(), vector3([1.0, 6.0, 0.2]));
    properties.insert("InitialSize".to_string(), vector3([1.0, 6.0, 0.2]));
    properties.insert("CFrame".to_string(), cframe([0.0; 3], IDENTITY));
    properties.insert("Material".to_string(), Variant::Enum(SMOOTH_PLASTIC));
    properties.insert(
        "Color3uint8".to_string(),
        Variant::Color3uint8 { r: 0, g: 255, b: 0 },
    );
    dom.insert(upper);

    let mut lower = Instance::new(lower_ref, "Part", "LowerArm");
    let properties = lower.properties_mut();
    properties.insert("size".to_string(), vector3([1.0, 3.0, 0.2]));
    properties.insert("CFrame".to_string(), cframe([0.0, -1.5, 0.0], IDENTITY));
    properties.insert("Transparency".to_string(), Variant::Float32(1.0));
    properties.insert("Material".to_string(), Variant::Enum(SMOOTH_PLASTIC));
    dom.insert(lower);

    dom.set_parent(model, Some(Ref::new(WORKSPACE)));
    dom.set_parent(upper_ref, Some(model));
    dom.set_parent(lower_ref, Some(model));
    (model, lower_ref)
}

fn workspace() -> WeakDom {
    let mut dom = WeakDom::new();
    dom.insert(Instance::new(Ref::new(WORKSPACE), "Workspace", "Workspace"));
    dom.set_parent(Ref::new(WORKSPACE), None);
    dom
}

/// The arm in a place of its own, as opened.
fn place() -> WeakDom {
    let mut dom = workspace();
    insert_arm(&mut dom, ARM);
    dom.take_changes();
    dom
}

/// Writes `lower`'s `CFrame` and patches it in — a quarter turn about +Z
/// around the joint at the origin when `bent` (its centre swings from
/// (0, −1.5) to (1.5, 0)), hanging at rest otherwise.
fn turn(headless: &mut Headless, dom: &mut WeakDom, lower: Ref, bent: bool) {
    let cframe = match bent {
        true => cframe([1.5, 0.0, 0.0], QUARTER_Z),
        false => cframe([0.0, -1.5, 0.0], IDENTITY),
    };
    dom.set_property(lower, "CFrame", cframe)
        .expect("a part takes a CFrame");
    let changes = dom.take_changes();
    assert_eq!(
        headless.apply_changes(dom, &changes).expect("applies"),
        Applied::Patched,
        "a pose is a patch, not a reload"
    );
}

fn assert_at_rest(pixels: &[u8]) {
    assert!(count(pixels, |_, _| true) > 200, "the ribbon is drawn");
    assert!(
        count(pixels, below_the_joint) > 20,
        "at rest the lower half hangs below the joint"
    );
    assert_eq!(count(pixels, out_to_the_side), 0, "and nothing sticks out");
}

fn assert_bent(pixels: &[u8]) {
    assert_eq!(
        count(pixels, below_the_joint),
        0,
        "the lower half no longer hangs below the joint"
    );
    assert!(
        count(pixels, out_to_the_side) > 20,
        "it sticks out to the side instead"
    );
    assert!(
        count(pixels, |row, _| row < 0.45) > 20,
        "the upper half is still drawn above the joint"
    );
}

fn headless() -> Option<Headless> {
    // Once per run: the tests run side by side, and two writes of the same
    // cache entry at once trip over each other's temporary file.
    static CACHED: std::sync::Once = std::sync::Once::new();
    CACHED.call_once(|| {
        let cache = rbx_assets::AssetCache::new(None).expect("the asset cache opens");
        cache
            .put_native(NATIVE_PATH, &ribbon())
            .expect("the mesh is cached");
    });
    match Headless::load(&fixture(), false) {
        Ok(headless) => Some(headless),
        Err(why) => {
            println!("skipped: no GPU to render with ({why})");
            None
        }
    }
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
    let Some(mut headless) = headless() else {
        return;
    };
    let mut dom = place();
    headless.reload(&dom).expect("the DOM rebuilds");
    settle(&mut headless);
    headless.open_at([0.0, 0.0, -9.0], [0.0, 0.0, 0.0], 70.0);

    let rest = frame(&mut headless);
    assert_at_rest(&rest);
    let lower = Ref::new(ARM + 2);
    turn(&mut headless, &mut dom, lower, true);
    assert_bent(&frame(&mut headless));
    // And back: the rest frame exactly.
    turn(&mut headless, &mut dom, lower, false);
    assert_eq!(frame(&mut headless), rest, "the rest pose draws as before");
}

/// What the auto-rigger does: the rig arrives after the place is open, as
/// one batch of inserts, and every edit swaps the whole `Model` for a new
/// one — each of which has to bend.
#[test]
fn a_rig_inserted_after_load_and_then_replaced_bends_each_time() {
    let Some(mut headless) = headless() else {
        return;
    };
    let mut dom = workspace();
    // Something to open: a place with no part at all does not load. Behind
    // the camera, out of every frame compared.
    let mut anchor = Instance::new(Ref::new(5), "Part", "Anchor");
    let properties = anchor.properties_mut();
    properties.insert("size".to_string(), vector3([1.0; 3]));
    properties.insert("CFrame".to_string(), cframe([0.0, 0.0, -100.0], IDENTITY));
    properties.insert("Material".to_string(), Variant::Enum(SMOOTH_PLASTIC));
    dom.insert(anchor);
    dom.set_parent(Ref::new(5), Some(Ref::new(WORKSPACE)));
    dom.take_changes();
    headless.reload(&dom).expect("the DOM rebuilds");
    headless.open_at([0.0, 0.0, -9.0], [0.0, 0.0, 0.0], 70.0);
    assert_eq!(count(&frame(&mut headless), |_, _| true), 0, "nothing yet");

    // Inserted: its mesh is not resident yet, so it streams in.
    let (model, lower) = insert_arm(&mut dom, 10);
    let changes = dom.take_changes();
    assert_eq!(
        headless.apply_changes(&dom, &changes).expect("applies"),
        Applied::Patched,
        "an inserted rig is a patch"
    );
    settle(&mut headless);
    assert_at_rest(&frame(&mut headless));
    turn(&mut headless, &mut dom, lower, true);
    assert_bent(&frame(&mut headless));

    // Replaced, in one batch: the mesh is resident now, so the new rig
    // binds as it is patched in.
    dom.remove(model);
    let (model, lower) = insert_arm(&mut dom, 20);
    let changes = dom.take_changes();
    assert_eq!(
        headless.apply_changes(&dom, &changes).expect("applies"),
        Applied::Patched,
        "a replaced rig is a patch"
    );
    settle(&mut headless);
    assert_at_rest(&frame(&mut headless));
    turn(&mut headless, &mut dom, lower, true);
    assert_bent(&frame(&mut headless));

    // And once more, this time replaced while its predecessor is bent.
    dom.remove(model);
    let (_, lower) = insert_arm(&mut dom, 30);
    let changes = dom.take_changes();
    headless.apply_changes(&dom, &changes).expect("applies");
    settle(&mut headless);
    assert_at_rest(&frame(&mut headless));
    turn(&mut headless, &mut dom, lower, true);
    assert_bent(&frame(&mut headless));
}

/// Pixels of `a` and `b` that differ at all, among those whose row and
/// column pass `inside`.
fn changed(a: &[u8], b: &[u8], inside: impl Fn(f32, f32) -> bool) -> usize {
    let (width, height) = (SIZE.0 as usize, SIZE.1 as usize);
    (0..height)
        .flat_map(|row| (0..width).map(move |column| (row, column)))
        .filter(|&(row, column)| {
            let at = (row * width + column) * 4;
            a[at..at + 4] != b[at..at + 4]
                && inside(
                    (row as f32 + 0.5) / height as f32,
                    (column as f32 + 0.5) / width as f32,
                )
        })
        .count()
}

/// The bent arm drawn with the pose deformed on the CPU and with it written
/// by the GPU's skinning pass: the same picture, bar rounding.
#[test]
fn the_gpu_bends_the_mesh_as_the_cpu_does() {
    let Some(mut headless) = headless() else {
        return;
    };
    if !headless.skins_on_gpu() {
        println!("skipped: this device skins on the CPU only");
        return;
    }
    let mut dom = place();
    headless.reload(&dom).expect("the DOM rebuilds");
    settle(&mut headless);
    headless.open_at([0.0, 0.0, -9.0], [0.0, 0.0, 0.0], 70.0);
    turn(&mut headless, &mut dom, Ref::new(ARM + 2), true);

    let gpu = frame(&mut headless);
    assert_bent(&gpu);
    headless.set_skins_on_gpu(false);
    assert!(!headless.skins_on_gpu());
    let cpu = frame(&mut headless);
    assert_bent(&cpu);
    let differing = changed(&gpu, &cpu, |_, _| true);
    assert!(
        differing <= 8,
        "{differing} pixels differ between the GPU and the CPU pose"
    );
    // And a pose taken while on the CPU carries over to the GPU.
    turn(&mut headless, &mut dom, Ref::new(ARM + 2), false);
    headless.set_skins_on_gpu(true);
    assert_at_rest(&frame(&mut headless));
}

/// A selected skinned mesh is outlined where it is drawn, bent, not where
/// its rest pose would be: the selection mask is posed with it, on either
/// path.
#[test]
fn a_selected_bent_mesh_is_cued_where_it_bends() {
    let Some(mut headless) = headless() else {
        return;
    };
    for on_gpu in [true, false] {
        headless.set_skins_on_gpu(on_gpu);
        let mut dom = place();
        headless.reload(&dom).expect("the DOM rebuilds");
        settle(&mut headless);
        headless.open_at([0.0, 0.0, -9.0], [0.0, 0.0, 0.0], 70.0);
        turn(&mut headless, &mut dom, Ref::new(ARM + 2), true);
        let bare = frame(&mut headless);
        headless.set_selection(&[rbx_viewer::pick::Selected::read(
            &dom,
            &rbx_reflection::ReflectionDatabase::embedded(),
            Ref::new(ARM + 1),
        )]);
        let cued = frame(&mut headless);
        headless.set_selection(&[]);

        let path = if on_gpu { "GPU" } else { "CPU" };
        let aside = changed(&bare, &cued, out_to_the_side);
        assert!(
            aside > 20,
            "{path}: only {aside} pixels of cue around the bent lower half"
        );
        let below = changed(&bare, &cued, below_the_joint);
        assert!(
            below <= 4,
            "{path}: {below} pixels of cue below the joint, where the rest pose hangs"
        );
    }
}
