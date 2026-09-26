//! Viewer for Roblox place and model files: every BasePart is drawn as a small
//! procedural mesh matching its shape (box, ball, cylinder, wedge or corner
//! wedge — see [`shapes`]), `MeshPart`/`SpecialMesh` FileMesh instances get
//! their real downloaded geometry (see `scene::filemesh`), and decals,
//! textures and the sky are painted on top. Everything is then lit by the
//! place's own `Lighting` service (see [`lighting`]).
//!
//! Entry point: [`run`], which either opens a free-flight window (or, with
//! `--orbit`, an orbiting one) or writes a single offscreen frame to a PNG,
//! depending on [`Options`]. Embedders that draw the place inside a UI of
//! their own use [`Headless`] instead, feeding it [`CameraInput`] so their
//! viewport flies exactly like the window does.
//!
//! A place or model file is either Roblox's binary format or its XML one
//! (`.rbxlx`/`.rbxmx`); [`read_place`] sniffs the bytes and dispatches to
//! `rbx_binary` or `rbx_xml` accordingly, so every entry point that opens a
//! file — including embedders such as `rbxstudio` — shares the one dispatch.

// The browser build (`web`) draws and streams, and nothing more: the editor's
// half of this crate — edits patched in place, picking, the screenshot and
// batch paths — has no caller there.
#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]

#[cfg(not(target_arch = "wasm32"))]
mod app;
mod assets;
#[cfg(not(target_arch = "wasm32"))]
mod batch;
mod camera;
#[cfg(not(target_arch = "wasm32"))]
mod capture;
mod changes;
mod cli;
mod controller;
mod fonts;
mod fps;
pub mod gizmo;
mod gpu;
#[cfg(not(target_arch = "wasm32"))]
mod headless;
mod input;
mod lighting;
mod load;
mod pacing;
pub mod pick;
mod quality;
mod renderer;
mod scene;
#[cfg(not(target_arch = "wasm32"))]
mod serve;
pub mod services;
mod shapes;
pub mod snap;
mod textures;
mod view;
#[cfg(target_arch = "wasm32")]
mod web;

pub use camera::Pose;
#[cfg(not(target_arch = "wasm32"))]
pub use capture::Rendered;
pub use changes::{Applied, Rebuild};
pub use cli::Options;
pub use controller::CameraFeel;
pub use gizmo::Gizmo;
// Only so a report (`examples/bench`) can label its numbers with the GPU that
// produced them; nothing in the render path itself asks for this.
pub use gpu::describe_adapter;
#[cfg(not(target_arch = "wasm32"))]
pub use headless::{GuiCanvas, Headless};
pub use input::{CameraInput, CameraKey};
pub use lighting::light_guides;
// An editor placing the sun by pointing at the scene needs the inverse of the
// sun model the renderer lights with.
pub use lighting::sun;
pub use load::read_place;
pub use quality::{FrameRateManager, QualityLevel};
pub use renderer::{GuiBox, Segment};
pub use scene::{resolved_shape_label, ScrollTarget};
#[cfg(not(target_arch = "wasm32"))]
pub use serve::{serve, DEFAULT_PORT};
// `rbxview`'s own title bar and `rbxstudio`'s Viewport dock read fps the same
// way; shared here rather than each crate carrying its own copy of the format.
pub use fps::readout as fps_readout;

#[cfg(not(target_arch = "wasm32"))]
use glam::Vec3;
#[cfg(not(target_arch = "wasm32"))]
use load::{Loaded, Toggles};

#[cfg(not(target_arch = "wasm32"))]
pub fn run(options: &Options) -> Result<(), String> {
    if let Some(out_dir) = options.batch() {
        return batch::run(options, out_dir);
    }

    let loaded = Loaded::read(options.path(), toggles(options))?;

    let eye_look_at = options
        .eye_look_at()
        .map(|(eye, look_at)| (Vec3::from(eye), Vec3::from(look_at)));

    match options.screenshot() {
        Some(output) => {
            // Not part of the quality table (see `QualityProfile::particles`):
            // `--no-particles` is a plain on/off switch at every level, not a
            // step on the ladder `QualityLevel::profile` builds.
            let mut profile = options.quality().profile();
            profile.particles = options.particles();
            profile.beams = options.beams();
            profile.trails = options.trails();
            profile.gui = options.gui();
            capture::write_png(
                loaded.world(),
                &profile,
                output,
                options.size(),
                capture::Framing {
                    yaw: options.yaw(),
                    pitch: options.pitch(),
                    zoom: options.zoom(),
                    eye_look_at,
                    orthographic: options.orthographic(),
                    elapsed: options.elapsed(),
                },
            )
        }
        None => app::open_window(
            loaded.world(),
            options.quality(),
            &options.title(),
            options.orbit(),
            options.speed(),
            options.sensitivity(),
        ),
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn toggles(options: &Options) -> Toggles {
    Toggles {
        textures: options.textures(),
        materials: options.materials(),
        lights: options.lights(),
        clock_time: options.clock_time(),
        show_development_gui: options.show_development_gui(),
    }
}
