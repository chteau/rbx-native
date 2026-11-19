//! The render fidelity harness: small scenes, each with a fixed camera,
//! drawn by the real Roblox client and by this renderer, and compared.
//!
//! One [`Scene`] is the single source of both pictures. [`Scene::place`] is
//! the place this renderer draws, holding only what the scene sets (so a
//! property it leaves out must fall back to what Roblox does with it).
//! [`roblox::place`] packs every scene into one place for the real client: a
//! client script swaps each scene in, sets `Lighting`, points the camera and
//! prints when the frame is ready to capture.
//!
//! References are the real client's frames, downsampled to [`COMPARE`] and
//! kept under `crates/rbx_viewer/fidelity/refs/`; `fidelity/goldens.json`
//! holds the metrics this renderer reached against each and how far they may
//! slip. `tests/fidelity.rs` fails on a slip, `examples/fidelity.rs` writes
//! the report and the side-by-side pictures, and `rbx_sim`'s
//! `record_render_goldens` example captures new references.

pub mod metrics;
pub mod roblox;
mod scenes;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rbx_dom::{CFrameData, Color3Data, Content, Ref, Variant, Vector3Data, WeakDom};

pub use scenes::scenes;

/// The size frames are captured and drawn at.
pub const CAPTURE: (u32, u32) = (1280, 720);
/// The size frames are compared at: a 4 × 4 box average of [`CAPTURE`], so
/// both sides' edge antialiasing and texture sampling wash out alike.
pub const COMPARE: (u32, u32) = (320, 180);
/// The rows of the top of the frame Roblox keeps its own top bar in (the
/// menu and chat buttons a game cannot hide), left out of every metric.
pub const TOP_BAR: f32 = 0.085;

/// One fidelity scene.
pub struct Scene {
    pub name: &'static str,
    pub about: &'static str,
    pub eye: [f32; 3],
    pub look_at: [f32; 3],
    pub fov: f32,
    /// `Lighting` properties the scene sets. Every other scriptable one is
    /// left at its class default (the real client resets them per scene).
    pub lighting: Vec<(&'static str, Variant)>,
    /// Named points of the world (a face's centre, a patch of floor, a far
    /// point for the sky) whose colours the report lists side by side.
    pub probes: Vec<(&'static str, [f32; 3])>,
    build: fn(&mut Builder),
}

impl Scene {
    /// Where `point` lands in a [`CAPTURE`]-shaped frame, as fractions of
    /// its width and height from the top left; `None` behind the eye.
    pub fn project(&self, point: [f32; 3]) -> Option<[f32; 2]> {
        let eye = glam::Vec3::from(self.eye);
        let forward = (glam::Vec3::from(self.look_at) - eye).normalize();
        let right = forward.cross(glam::Vec3::Y).normalize();
        let up = right.cross(forward);
        let d = glam::Vec3::from(point) - eye;
        let depth = d.dot(forward);
        if depth <= 0.0 {
            return None;
        }
        let half = (self.fov.to_radians() / 2.0).tan();
        let aspect = CAPTURE.0 as f32 / CAPTURE.1 as f32;
        let x = d.dot(right) / (depth * half * aspect);
        let y = d.dot(up) / (depth * half);
        Some([0.5 + x / 2.0, 0.5 - y / 2.0])
    }

    /// A rectangle three [`COMPARE`] pixels wide around where `point` lands.
    pub fn probe_rect(&self, point: [f32; 3]) -> Option<[f32; 4]> {
        let [x, y] = self.project(point)?;
        if !(0.0..1.0).contains(&x) || !(TOP_BAR..1.0).contains(&y) {
            return None;
        }
        let (dx, dy) = (1.5 / COMPARE.0 as f32, 1.5 / COMPARE.1 as f32);
        Some([x - dx, y - dy, x + dx, y + dy])
    }

    /// The place this renderer draws: `Workspace` with the scene's world
    /// and `Lighting` with only what the scene sets.
    pub fn place(&self) -> WeakDom {
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        let lighting = dom.new_instance("Lighting", "Lighting", None);
        for (key, value) in &self.lighting {
            dom.set_property(lighting, key, value.clone())
                .expect("Lighting exists");
        }
        let mut builder = Builder {
            dom,
            world: workspace,
            lighting,
        };
        (self.build)(&mut builder);
        let mut dom = builder.dom;
        dom.take_changes();
        dom
    }

    /// Builds the scene's world under `world` and its `Lighting` children
    /// under `lighting`, in a DOM of the caller's.
    pub fn build_into(&self, dom: WeakDom, world: Ref, lighting: Ref) -> WeakDom {
        let mut builder = Builder {
            dom,
            world,
            lighting,
        };
        (self.build)(&mut builder);
        builder.dom
    }
}

/// What a scene's `build` adds instances through.
pub struct Builder {
    dom: WeakDom,
    world: Ref,
    lighting: Ref,
}

impl Builder {
    /// An anchored `Part` of `shape` (Enum.PartType) in the scene's world.
    pub fn part(
        &mut self,
        shape: u32,
        size: [f32; 3],
        cframe: CFrameData,
        rgb: [u8; 3],
        material: u32,
    ) -> Ref {
        let part = self.dom.new_instance("Part", "Part", Some(self.world));
        self.set(part, "Anchored", Variant::Bool(true));
        self.set(part, "shape", Variant::Enum(shape));
        self.set(part, "size", vec3(size));
        self.set(part, "CFrame", Variant::CFrame(cframe));
        let [r, g, b] = rgb;
        self.set(part, "Color3uint8", Variant::Color3uint8 { r, g, b });
        self.set(part, "Material", Variant::Enum(material));
        // Smooth, as Studio has made parts since 2018: `Part`'s class
        // defaults are studs on top and inlets below.
        self.set(part, "TopSurface", Variant::Enum(0));
        self.set(part, "BottomSurface", Variant::Enum(0));
        part
    }

    /// A plastic block at `position`.
    pub fn block(&mut self, size: [f32; 3], position: [f32; 3], rgb: [u8; 3]) -> Ref {
        self.part(BLOCK, size, at(position), rgb, PLASTIC)
    }

    pub fn child(&mut self, parent: Ref, class: &str) -> Ref {
        self.dom.new_instance(class, class, Some(parent))
    }

    /// An instance under the scene's `Lighting` (a `Sky`, `Atmosphere` or
    /// post effect).
    pub fn lighting_child(&mut self, class: &str) -> Ref {
        let lighting = self.lighting;
        self.child(lighting, class)
    }

    pub fn set(&mut self, instance: Ref, key: &str, value: Variant) {
        self.dom
            .set_property(instance, key, value)
            .expect("the instance exists");
    }

    /// The Baseplate template's floor without its stud texture: 2048 studs
    /// square, its top at y = 0, Roblox's 91 grey.
    pub fn floor(&mut self) {
        self.block([2048.0, 16.0, 2048.0], [0.0, -8.0, 0.0], [91, 91, 91]);
    }
}

pub const BALL: u32 = 0;
pub const BLOCK: u32 = 1;
pub const CYLINDER: u32 = 2;
pub const WEDGE: u32 = 3;
pub const PLASTIC: u32 = 256;

pub fn at([x, y, z]: [f32; 3]) -> CFrameData {
    CFrameData {
        position: Vector3Data { x, y, z },
        rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    }
}

/// `at(position)` turned `degrees` about the X axis (`CFrame.Angles(a, 0, 0)`).
pub fn tilted_x([x, y, z]: [f32; 3], degrees: f32) -> CFrameData {
    let (s, c) = degrees.to_radians().sin_cos();
    CFrameData {
        position: Vector3Data { x, y, z },
        rotation: [1.0, 0.0, 0.0, 0.0, c, -s, 0.0, s, c],
    }
}

pub fn vec3([x, y, z]: [f32; 3]) -> Variant {
    Variant::Vector3(Vector3Data { x, y, z })
}

pub fn rgb(r: u8, g: u8, b: u8) -> Variant {
    Variant::Color3(Color3Data {
        r: f32::from(r) / 255.0,
        g: f32::from(g) / 255.0,
        b: f32::from(b) / 255.0,
    })
}

pub fn float(value: f32) -> Variant {
    Variant::Float32(value)
}

pub fn content(uri: &str) -> Variant {
    Variant::Content(Content::Uri(uri.to_owned()))
}

/// Draws `scene` as this renderer does at [`CAPTURE`], once every asset it
/// asked for has landed: RGBA8, row by row, and the warnings loading it
/// raised (an asset that could not be fetched, say).
pub fn render(scene: &Scene) -> Result<(Vec<u8>, Vec<String>), String> {
    const DRAIN: usize = 24;
    const LIMIT: Duration = Duration::from_secs(90);
    let mut headless = crate::Headless::from_dom(&scene.place(), true)?;
    headless.open_at(scene.eye, scene.look_at, scene.fov);
    let started = Instant::now();
    loop {
        let swapped = headless.swap_assets();
        if headless.assets_in_flight() == 0 && !swapped {
            break;
        }
        if started.elapsed() > LIMIT {
            return Err(format!(
                "{}: assets still loading after {LIMIT:?}",
                scene.name
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let (width, height) = CAPTURE;
    for _ in 0..DRAIN {
        headless.render_frame(width, height)?;
    }
    let pixels = headless.take_frame()?.ok_or("no frame was queued")?.pixels;
    Ok((pixels, headless.drain_warnings()))
}

/// Where the committed references and goldens live.
pub fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fidelity")
}

pub fn reference_path(name: &str) -> PathBuf {
    dir().join("refs").join(format!("{name}.png"))
}

/// Reads an RGBA8 (or RGB8) PNG as RGBA8.
pub fn read_png(path: &Path) -> Result<(u32, u32, Vec<u8>), String> {
    let file = std::fs::File::open(path).map_err(|err| format!("{}: {err}", path.display()))?;
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .map_err(|err| format!("{}: {err}", path.display()))?;
    let mut buffer = vec![0; reader.output_buffer_size().ok_or("PNG too large")?];
    let info = reader
        .next_frame(&mut buffer)
        .map_err(|err| format!("{}: {err}", path.display()))?;
    buffer.truncate(info.buffer_size());
    let pixels = match info.color_type {
        png::ColorType::Rgba => buffer,
        png::ColorType::Rgb => buffer
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|[r, g, b]| [*r, *g, *b, 255])
            .collect(),
        other => return Err(format!("{}: unsupported {other:?}", path.display())),
    };
    Ok((info.width, info.height, pixels))
}

pub fn write_png(path: &Path, (width, height): (u32, u32), rgba: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| format!("{}: {err}", parent.display()))?;
    }
    let file = std::fs::File::create(path).map_err(|err| format!("{}: {err}", path.display()))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::High);
    encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(rgba))
        .map_err(|err| format!("{}: {err}", path.display()))
}
