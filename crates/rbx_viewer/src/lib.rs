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

mod app;
mod assets;
mod camera;
mod capture;
mod cli;
mod controller;
pub mod gizmo;
mod gpu;
mod headless;
mod input;
mod lighting;
mod load;
pub mod pick;
mod quality;
mod renderer;
mod scene;
mod shapes;
mod textures;
mod view;

pub use camera::Pose;
pub use capture::Rendered;
pub use cli::Options;
pub use gizmo::Gizmo;
pub use headless::Headless;
pub use input::{CameraInput, CameraKey};
pub use load::read_place;
pub use quality::{FrameRateManager, QualityLevel};

use glam::Vec3;
use load::{Loaded, Toggles};

pub fn run(options: &Options) -> Result<(), String> {
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
                    eye_look_at,
                    orthographic: options.orthographic(),
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

fn toggles(options: &Options) -> Toggles {
    Toggles {
        textures: options.textures(),
        materials: options.materials(),
        lights: options.lights(),
        clock_time: options.clock_time(),
    }
}
