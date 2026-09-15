//! Command line arguments for `rbxview`.

use std::path::{Path, PathBuf};

use crate::controller::{
    DEFAULT_SENSITIVITY, DEFAULT_SPEED, LARGE_SCENE_RADIUS, LARGE_SCENE_SPEED_DIVISOR,
};
use crate::quality::QualityLevel;

const DEFAULT_SIZE: (u32, u32) = (1280, 720);

/// An X,Y,Z position in studs, kept crate-agnostic (no `glam` dependency here) since
/// `Options` only ever hands it back unchanged for the caller to interpret.
type Position = (f32, f32, f32);

/// A parsed command line: the file to show, and where to write a single frame instead
/// of opening a window.
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    path: PathBuf,
    screenshot: Option<PathBuf>,
    size: (u32, u32),
    yaw: Option<i32>,
    pitch: Option<i32>,
    eye: Option<Position>,
    look_at: Option<Position>,
    textures: bool,
    materials: bool,
    lights: bool,
    particles: bool,
    beams: bool,
    trails: bool,
    gui: bool,
    orbit: bool,
    speed: Option<f32>,
    sensitivity: f32,
    clock_time: Option<f32>,
    quality: QualityLevel,
    orthographic: bool,
}

impl Options {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut path = None;
        let mut screenshot = None;
        let mut size = DEFAULT_SIZE;
        let mut yaw = None;
        let mut pitch = None;
        let mut eye = None;
        let mut look_at = None;
        let mut textures = true;
        let mut materials = true;
        let mut lights = true;
        let mut particles = true;
        let mut beams = true;
        let mut trails = true;
        let mut gui = true;
        let mut orbit = false;
        let mut speed = None;
        let mut sensitivity = DEFAULT_SENSITIVITY;
        let mut clock_time = None;
        let mut quality = QualityLevel::default();
        let mut orthographic = false;

        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--screenshot" => screenshot = Some(PathBuf::from(value(&mut args, &arg)?)),
                "--size" => size = parse_size(&value(&mut args, &arg)?)?,
                "--yaw" => yaw = Some(parse_angle(&value(&mut args, &arg)?)?),
                "--pitch" => pitch = Some(parse_angle(&value(&mut args, &arg)?)?),
                "--eye" => eye = Some(parse_vec3(&value(&mut args, &arg)?)?),
                "--look-at" => look_at = Some(parse_vec3(&value(&mut args, &arg)?)?),
                "--no-textures" => textures = false,
                "--no-materials" => materials = false,
                "--no-lights" => lights = false,
                "--no-particles" => particles = false,
                "--no-beams" => beams = false,
                "--no-trails" => trails = false,
                "--no-gui" => gui = false,
                "--orbit" => orbit = true,
                "--speed" => speed = Some(parse_positive(&value(&mut args, &arg)?)?),
                "--sensitivity" => sensitivity = parse_positive(&value(&mut args, &arg)?)?,
                "--clock-time" => clock_time = Some(parse_hours(&value(&mut args, &arg)?)?),
                "--quality" => quality = value(&mut args, &arg)?.parse()?,
                "--orthographic" => orthographic = true,
                flag if flag.starts_with('-') => return Err(format!("unknown option '{flag}'")),
                positional if path.is_none() => path = Some(PathBuf::from(positional)),
                extra => return Err(format!("unexpected extra argument '{extra}'")),
            }
        }

        if eye.is_some() != look_at.is_some() {
            return Err("'--eye' and '--look-at' must be given together".to_string());
        }

        Ok(Options {
            path: path.ok_or_else(|| "missing input file".to_string())?,
            screenshot,
            size,
            yaw,
            pitch,
            eye,
            look_at,
            textures,
            materials,
            lights,
            particles,
            beams,
            trails,
            gui,
            orbit,
            speed,
            sensitivity,
            clock_time,
            quality,
            orthographic,
        })
    }

    pub fn usage(program: &str) -> String {
        format!(
            "usage: {program} <file.rbxm|file.rbxl> [--screenshot <out.png>] [--size WxH]\n\
             \x20              [--yaw <degrees>] [--pitch <degrees>]\n\
             \x20              [--eye X,Y,Z --look-at X,Y,Z] [--no-textures] [--no-materials]\n\
             \x20              [--no-lights] [--no-particles] [--no-beams] [--no-trails]\n\
             \x20              [--no-gui]\n\
             \x20              [--orbit] [--speed <studs/s>] [--sensitivity <deg/px>]\n\
             \x20              [--clock-time <hours>] [--quality <auto|1..21>]\n\
             \x20              [--orthographic]\n\
             \x20 --screenshot  render a single frame offscreen to a PNG and exit\n\
             \x20 --size        pixel size of that frame (default {}x{})\n\
             \x20 --yaw         angle to look from, in degrees (screenshots only)\n\
             \x20 --pitch       height to look from, in degrees above the scene;\n\
             \x20               negative looks up from underneath (screenshots only)\n\
             \x20 --eye         camera position in studs, e.g. 1,2,3 — overrides the\n\
             \x20               orbit framing entirely; requires --look-at (screenshots only)\n\
             \x20 --look-at     point the camera looks toward, in studs; requires --eye\n\
             \x20 --no-textures skip downloading decals, textures and the sky\n\
             \x20 --no-materials skip downloading the material texture packs, so every\n\
             \x20               part is drawn as plain plastic in its own colour\n\
             \x20 --no-lights   skip the place's own PointLights, SpotLights and\n\
             \x20               SurfaceLights, leaving only the sun, moon and sky\n\
             \x20 --no-particles skip every ParticleEmitter and downloading their textures\n\
             \x20 --no-beams    skip every Beam and downloading their textures\n\
             \x20 --no-trails   skip every Trail and downloading their textures\n\
             \x20 --no-gui      skip every ScreenGui, BillboardGui and SurfaceGui\n\
             \x20 --orbit       auto-orbit the scene until the first input, instead of\n\
             \x20               spawning the free camera at its center right away\n\
             \x20 --speed       free camera speed in studs/second (default {}, or scene\n\
             \x20               radius / {} once that radius exceeds {} studs)\n\
             \x20 --sensitivity mouse-look sensitivity in degrees per pixel (default {})\n\
             \x20 --clock-time  hour of the day to light the scene at, 0 to 24 and\n\
             \x20               fractional (18.5 is 18:30), overriding the place's own\n\
             \x20               TimeOfDay/ClockTime; below the horizon the moon lights it\n\
             \x20 --quality     Roblox's own graphics quality level, {} to {}, or 'auto'\n\
             \x20               (default {}): lower levels drop shadows, post effects,\n\
             \x20               local lights, reflections, texture detail and view\n\
             \x20               distance, in that rough order. 'auto' lowers the level\n\
             \x20               itself while the window is missing its frame budget\n\
             \x20 --orthographic draw with a parallel projection instead of perspective,\n\
             \x20               framed at the same apparent scale (screenshots only)",
            DEFAULT_SIZE.0,
            DEFAULT_SIZE.1,
            DEFAULT_SPEED,
            LARGE_SCENE_SPEED_DIVISOR,
            LARGE_SCENE_RADIUS,
            DEFAULT_SENSITIVITY,
            QualityLevel::MIN,
            QualityLevel::MAX,
            QualityLevel::MAX
        )
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn screenshot(&self) -> Option<&Path> {
        self.screenshot.as_deref()
    }

    pub(crate) fn size(&self) -> (u32, u32) {
        self.size
    }

    pub(crate) fn yaw(&self) -> Option<f32> {
        self.yaw.map(|degrees| degrees as f32)
    }

    pub(crate) fn pitch(&self) -> Option<f32> {
        self.pitch.map(|degrees| degrees as f32)
    }

    /// `--eye`/`--look-at` are validated together at parse time: either both
    /// are `Some` or both are `None`, so callers only ever need to check one.
    pub(crate) fn eye_look_at(&self) -> Option<(Position, Position)> {
        self.eye.zip(self.look_at)
    }

    pub(crate) fn textures(&self) -> bool {
        self.textures
    }

    pub(crate) fn materials(&self) -> bool {
        self.materials
    }

    /// False skips every `PointLight`/`SpotLight`/`SurfaceLight` in the place,
    /// which is what makes an A/B capture of them possible.
    pub(crate) fn lights(&self) -> bool {
        self.lights
    }

    /// False skips every `ParticleEmitter`, and the network fetch of their
    /// textures with it.
    pub(crate) fn particles(&self) -> bool {
        self.particles
    }

    /// False skips every `Beam`, and the network fetch of their textures with it.
    pub(crate) fn beams(&self) -> bool {
        self.beams
    }

    /// False skips every `Trail`, and the network fetch of their textures with it.
    pub(crate) fn trails(&self) -> bool {
        self.trails
    }

    pub(crate) fn gui(&self) -> bool {
        self.gui
    }

    pub(crate) fn orbit(&self) -> bool {
        self.orbit
    }

    /// `None` when the user didn't pick one: the caller derives a scene-aware
    /// default once it knows the scene's bounds (see `controller::default_speed`).
    pub(crate) fn speed(&self) -> Option<f32> {
        self.speed
    }

    pub(crate) fn sensitivity(&self) -> f32 {
        self.sensitivity
    }

    /// `None` leaves the place's own `TimeOfDay`/`ClockTime` in charge.
    pub(crate) fn clock_time(&self) -> Option<f32> {
        self.clock_time
    }

    pub(crate) fn quality(&self) -> QualityLevel {
        self.quality
    }

    pub(crate) fn orthographic(&self) -> bool {
        self.orthographic
    }

    pub(crate) fn title(&self) -> String {
        let name = self
            .path
            .file_name()
            .unwrap_or(self.path.as_os_str())
            .to_string_lossy();
        format!("rbxview — {name}")
    }
}

fn value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("'{flag}' expects a value"))
}

// Whole degrees only: these options exist to pick another angle by hand, and a
// fractional one would only make the value harder to type back.
fn parse_angle(text: &str) -> Result<i32, String> {
    text.parse()
        .map_err(|_| format!("'{text}' is not a whole number of degrees"))
}

// Any hour, including a negative or a 30th: `Lighting` wraps the clock itself,
// and refusing 25 would only get in the way of a screenshot script stepping
// through a day.
fn parse_hours(text: &str) -> Result<f32, String> {
    let value: f32 = text
        .parse()
        .map_err(|_| format!("'{text}' is not a number of hours"))?;
    if !value.is_finite() {
        return Err(format!("'{text}' is not a number of hours"));
    }
    Ok(value)
}

fn parse_positive(text: &str) -> Result<f32, String> {
    let value: f32 = text
        .parse()
        .map_err(|_| format!("'{text}' is not a number"))?;
    if !value.is_finite() || value <= 0.0 {
        return Err(format!("'{text}' must be a positive number"));
    }
    Ok(value)
}

// Comma-separated, not space-separated: a single shell argument, matching `--size`.
fn parse_vec3(text: &str) -> Result<Position, String> {
    let invalid = || format!("'{text}' is not an X,Y,Z position");
    let mut parts = text.split(',');
    let (Some(x), Some(y), Some(z), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(invalid());
    };
    let x: f32 = x.trim().parse().map_err(|_| invalid())?;
    let y: f32 = y.trim().parse().map_err(|_| invalid())?;
    let z: f32 = z.trim().parse().map_err(|_| invalid())?;
    if !x.is_finite() || !y.is_finite() || !z.is_finite() {
        return Err(invalid());
    }
    Ok((x, y, z))
}

fn parse_size(text: &str) -> Result<(u32, u32), String> {
    let invalid = || format!("'{text}' is not a WxH pixel size");
    let (width, height) = text.split_once(['x', 'X']).ok_or_else(invalid)?;
    let width: u32 = width.parse().map_err(|_| invalid())?;
    let height: u32 = height.parse().map_err(|_| invalid())?;

    if width == 0 || height == 0 {
        return Err(format!("'{text}' has a zero dimension"));
    }
    Ok((width, height))
}

#[cfg(test)]
#[path = "cli/tests.rs"]
mod tests;
