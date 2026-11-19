//! A [`Headless`] with its scene, GPU device and decoded assets let go,
//! for a host that keeps a place open without showing it.

use rbx_dom::WeakDom;
use rbx_reflection::ReflectionDatabase;

use super::Headless;
use crate::camera::Viewpoint;
use crate::controller::{CameraFeel, Controller, Start, DEFAULT_SENSITIVITY};
use crate::lighting::Calibration;
use crate::load::Toggles;
use crate::quality::QualityLevel;
use crate::view::View;

/// Everything [`Parked::resume`] needs to draw the same picture again from
/// the place's DOM: the camera, the view (selection, gizmo, lines,
/// isolation), the quality and calibration. Small: the scene, the device
/// and every asset are gone.
pub struct Parked {
    quality: QualityLevel,
    /// The level the place fit in GPU memory at, so coming back does not
    /// run out all over again.
    cap: u8,
    calibration: Calibration,
    view: View,
    feel: CameraFeel,
    from: Viewpoint,
    speed: f32,
    toggles: Toggles,
    database: ReflectionDatabase,
    skinning_wanted: bool,
}

impl Headless {
    /// Drops the scene, its GPU device and its decoded assets, keeping what
    /// [`Parked::resume`] puts back.
    pub fn park(self) -> Parked {
        Parked {
            quality: self.quality,
            cap: self.rung.level,
            calibration: self.calibration,
            speed: self.controller.speed(),
            view: self.view,
            feel: self.feel,
            from: self.from,
            toggles: self.toggles,
            database: self.database,
            skinning_wanted: self.skinning_wanted,
        }
    }
}

impl Parked {
    /// Rebuilds the scene from `dom` on a new device, with the camera, view,
    /// quality and calibration it was parked with. Assets are asked for
    /// again, as on a first load. With no GPU able to draw, it comes back
    /// undrawn, as [`Headless::load_or_undrawn`] opens.
    pub fn resume(self, dom: &WeakDom) -> Result<Headless, String> {
        let mut headless = Headless::build(
            dom,
            self.toggles,
            self.database,
            self.view,
            self.cap,
            None,
            true,
        )?;
        headless.set_calibration(self.calibration);
        headless.set_quality(self.quality);
        headless.feel = self.feel;
        headless.from = self.from;
        let start = match self.from {
            Viewpoint::Orbit(_) => Start::Orbit,
            Viewpoint::Free(pose) => Start::Pose(pose),
        };
        headless.controller = Controller::new(
            start,
            &headless.bounds,
            Some(self.speed),
            DEFAULT_SENSITIVITY,
        );
        headless.controller.set_feel(self.feel);
        if !self.skinning_wanted {
            headless.set_skins_on_gpu(false);
        }
        Ok(headless)
    }
}
