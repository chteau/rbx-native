//! What the hardware can actually show, which is what the render loop paces
//! itself to (see [`crate::pacing`]).
//!
//! GPUI reports a display's bounds and scale but never its refresh rate, so it is
//! asked of RandR directly. Off X11 nothing is asked and the loop falls back to
//! 60 Hz.

/// The fastest refresh rate any attached output is running at, in Hz.
///
/// The fastest rather than the one under the window: which output a window sits
/// on is the compositor's business and moves while it is dragged, whereas capping
/// too high only wastes frames the reader can override with `RBX_STUDIO_FPS`.
pub(crate) fn refresh_hz() -> Option<f32> {
    #[cfg(target_os = "linux")]
    {
        x11_refresh_hz()
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

#[cfg(target_os = "linux")]
fn x11_refresh_hz() -> Option<f32> {
    use x11rb::connection::Connection as _;
    use x11rb::protocol::randr;

    let (connection, screen) = x11rb::connect(None).ok()?;
    let root = connection.setup().roots.get(screen)?.root;
    let resources = randr::get_screen_resources_current(&connection, root)
        .ok()?
        .reply()
        .ok()?;

    let rates = resources.crtcs.iter().filter_map(|crtc| {
        let info = randr::get_crtc_info(&connection, *crtc, resources.config_timestamp)
            .ok()?
            .reply()
            .ok()?;
        // A CRTC with no mode is an output that is attached but switched off.
        let mode = resources.modes.iter().find(|mode| mode.id == info.mode)?;
        refresh_rate(mode.dot_clock, mode.htotal, mode.vtotal)
    });

    rates.fold(None, |fastest: Option<f32>, hz| {
        Some(fastest.map_or(hz, |fastest| fastest.max(hz)))
    })
}

/// A mode's vertical refresh rate: the pixel clock spread over every pixel the
/// scanout walks through, blanking intervals included.
///
/// Interlaced and double-scan modes are not corrected for; no desktop display
/// reports one, and being wrong by a factor of two would only move the cap.
#[cfg(target_os = "linux")]
fn refresh_rate(dot_clock: u32, htotal: u16, vtotal: u16) -> Option<f32> {
    let pixels = u64::from(htotal) * u64::from(vtotal);
    if dot_clock == 0 || pixels == 0 {
        return None;
    }

    Some(dot_clock as f32 / pixels as f32)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::refresh_rate;

    // Real modes, as `xrandr --verbose` prints them: 1080p60 over HDMI, and the
    // 1080p75 this desktop's own panels run.
    #[test]
    fn a_mode_is_its_clock_over_the_pixels_it_scans() {
        let sixty = refresh_rate(148_500_000, 2200, 1125).expect("a rate");
        assert!((sixty - 60.0).abs() < 0.05, "{sixty}");

        let seventy_five = refresh_rate(174_500_000, 2080, 1119).expect("a rate");
        assert!((seventy_five - 74.97).abs() < 0.05, "{seventy_five}");
    }

    #[test]
    fn a_mode_that_scans_nothing_has_no_rate() {
        assert_eq!(refresh_rate(0, 2200, 1125), None);
        assert_eq!(refresh_rate(148_500_000, 0, 1125), None);
        assert_eq!(refresh_rate(148_500_000, 2200, 0), None);
    }
}
