//! screen_capture: a frame of the edited place from the viewport's render
//! thread, or of the running game from the embedded client's window.

use std::sync::mpsc::{Receiver, TryRecvError};

use gpui_kit::Context;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, RgbaImage};

use crate::mcp::{image_content, JobOutcome};

use super::super::play::Phase;
use super::super::Shell;

/// The longest side a capture is sent at.
pub(super) const MAX_SIDE: u32 = 1280;

type Pixels = Result<(u32, u32, Vec<u8>), String>;

/// Where a capture stands.
pub(super) enum Step {
    /// The viewport's render thread is drawing the frame.
    Frame(Receiver<Pixels>),
    /// The game's camera is being posed by the exec with this id.
    #[cfg_attr(
        not(any(target_os = "linux", windows, target_os = "macos")),
        allow(dead_code)
    )]
    Posed(u64),
}

impl Step {
    pub(super) fn ack(&self) -> Option<u64> {
        match self {
            Step::Posed(id) => Some(*id),
            Step::Frame(_) => None,
        }
    }
}

/// `(width, height)` with the longest side brought down to `MAX_SIDE`.
pub(super) fn scaled_size(width: u32, height: u32) -> (u32, u32) {
    let longest = width.max(height);
    if longest <= MAX_SIDE {
        return (width, height);
    }
    let scale = |side: u32| ((u64::from(side) * u64::from(MAX_SIDE)) / u64::from(longest)).max(1);
    (scale(width) as u32, scale(height) as u32)
}

/// Studio answers screen_capture with a JPEG of about this quality.
const JPEG_QUALITY: u8 = 90;

/// RGBA pixels as a JPEG, scaled down to `MAX_SIDE`.
pub(super) fn jpeg(width: u32, height: u32, rgba: Vec<u8>) -> Result<Vec<u8>, String> {
    let image = RgbaImage::from_raw(width, height, rgba)
        .ok_or_else(|| "the captured frame has the wrong size".to_owned())?;
    let (to_width, to_height) = scaled_size(width, height);
    let image = if (to_width, to_height) == (width, height) {
        image
    } else {
        image::imageops::resize(&image, to_width, to_height, FilterType::Triangle)
    };
    let rgb = DynamicImage::ImageRgba8(image).to_rgb8();
    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, JPEG_QUALITY)
        .encode_image(&rgb)
        .map_err(|err| format!("the capture could not be encoded: {err}"))?;
    Ok(bytes)
}

fn outcome(pixels: Pixels) -> JobOutcome {
    let (width, height, rgba) = pixels?;
    Ok(vec![image_content(&jpeg(width, height, rgba)?)])
}

#[cfg(any(target_os = "linux", windows, target_os = "macos"))]
fn lua_vector(v: [f32; 3]) -> String {
    format!("Vector3.new({}, {}, {})", v[0], v[1], v[2])
}

impl Shell {
    pub(super) fn capture_begin(
        &mut self,
        camera: Option<([f32; 3], [f32; 3])>,
        cx: &mut Context<Self>,
    ) -> Result<Step, String> {
        if self.play.phase != Phase::Running {
            return self
                .viewport
                .read(cx)
                .capture(camera)
                .map(Step::Frame)
                .ok_or_else(|| "the viewport is closed".to_owned());
        }
        #[cfg(any(target_os = "linux", windows, target_os = "macos"))]
        {
            if self.play_embedded().is_none() {
                return Err("the running game is not shown in the viewport".to_owned());
            }
            let Some((eye, at)) = camera else {
                let (sender, receiver) = std::sync::mpsc::channel();
                let _ = sender.send(self.embedded_pixels());
                return Ok(Step::Frame(receiver));
            };
            let code = format!(
                "local cam = workspace.CurrentCamera\n\
                 _G.__rbxnative_cam = {{ cam.CameraType, cam.CFrame }}\n\
                 cam.CameraType = Enum.CameraType.Scriptable\n\
                 cam.CFrame = CFrame.lookAt({}, {})\n\
                 task.wait(0.2)",
                lua_vector(eye),
                lua_vector(at)
            );
            match self.mcp_exec(rbx_play::protocol::Side::Client, &code)? {
                super::jobs::Wait::Ack(id) => Ok(Step::Posed(id)),
                _ => Err("the camera could not be posed".to_owned()),
            }
        }
        #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
        {
            let _ = camera;
            Err("capturing the running game is not supported on this system".to_owned())
        }
    }

    #[cfg(any(target_os = "linux", windows, target_os = "macos"))]
    fn embedded_pixels(&self) -> Pixels {
        let embedded = self
            .play_embedded()
            .ok_or_else(|| "the running game is not shown in the viewport".to_owned())?;
        embedded.capture().map_err(|err| err.to_string())
    }

    pub(super) fn capture_progress(
        &mut self,
        step: &mut Step,
        _cx: &mut Context<Self>,
    ) -> Option<JobOutcome> {
        match step {
            Step::Frame(receiver) => match receiver.try_recv() {
                Ok(pixels) => Some(outcome(pixels)),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => {
                    Some(Err("the viewport closed before the capture".to_owned()))
                }
            },
            Step::Posed(id) => {
                let posed = self.mcp_jobs.acks.remove(id)?;
                Some(self.capture_posed(posed))
            }
        }
    }

    #[cfg(any(target_os = "linux", windows, target_os = "macos"))]
    fn capture_posed(&mut self, posed: Result<String, String>) -> JobOutcome {
        posed?;
        let pixels = self.embedded_pixels();
        let restore = "local saved = _G.__rbxnative_cam\n\
                       if saved then\n\
                       local cam = workspace.CurrentCamera\n\
                       cam.CameraType = saved[1]\n\
                       cam.CFrame = saved[2]\n\
                       _G.__rbxnative_cam = nil\n\
                       end";
        if let Ok(body) = rbx_play::exec::command(rbx_play::protocol::Side::Client, restore) {
            self.play_send(body);
        }
        outcome(pixels)
    }

    #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
    fn capture_posed(&mut self, posed: Result<String, String>) -> JobOutcome {
        posed?;
        Err("capturing the running game is not supported on this system".to_owned())
    }
}
