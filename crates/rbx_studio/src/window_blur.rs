//! Who blurs behind a `"window": "blurred"` theme, and the backdrop the editor
//! draws itself where nobody does.
//!
//! Windows and macOS blur natively through GPUI, and so does KDE on Wayland.
//! On X11 the window carries `_KDE_NET_WM_BLUR_BEHIND_REGION`, which KWin and
//! picom honour. GNOME Shell on X11 blurs nothing, so there the editor
//! rebuilds what is behind each window (the windows below it, then the
//! wallpaper), blurs it on a worker thread and paints it under the ground.

#[cfg(target_os = "linux")]
mod capture;
#[cfg(target_os = "linux")]
mod wallpaper;
#[cfg(target_os = "linux")]
mod worker;
#[cfg(target_os = "linux")]
mod x11;

#[cfg(all(test, target_os = "linux"))]
mod tests;

use gpui_kit::{AnyView, App, Window};

/// Whether this desktop blurs behind a blurred theme's windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlurSupport {
    /// The platform or the compositor does it.
    Native,
    /// Nobody does, but the editor can paint it itself (GNOME on X11).
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Drawn,
    /// The window only shows see-through.
    None,
}

/// Decided once per run: the desktop doesn't change under a running editor.
pub(crate) fn support() -> BlurSupport {
    #[cfg(target_os = "linux")]
    {
        static SUPPORT: std::sync::OnceLock<BlurSupport> = std::sync::OnceLock::new();
        *SUPPORT.get_or_init(x11::detect)
    }
    #[cfg(not(target_os = "linux"))]
    BlurSupport::Native
}

/// Whether the active theme asks for blur this desktop can't give, the case
/// Settings explains.
pub(crate) fn blur_missing() -> bool {
    crate::theme::active().effects.window == gpui_kit::WindowBackgroundAppearance::Blurred
        && support() == BlurSupport::None
}

/// Sets or clears the blur hint on an X11 window to match the active theme.
pub(crate) fn sync_hint(window: &Window) {
    #[cfg(target_os = "linux")]
    if let Some(id) = crate::pointer_lock::window_id(window) {
        x11::set_hint(
            id,
            crate::theme::active().effects.window == gpui_kit::WindowBackgroundAppearance::Blurred,
        );
    }
    #[cfg(not(target_os = "linux"))]
    let _ = window;
}

/// What a window's `Root` holds: its view, over the drawn backdrop on a
/// desktop where the editor draws it (decided once, so a theme switched to
/// blurred later still gets one). Elsewhere the view itself.
pub(crate) fn wrap(view: impl Into<AnyView>, cx: &mut App) -> AnyView {
    let view = view.into();
    #[cfg(target_os = "linux")]
    if support() == BlurSupport::Drawn {
        use gpui_kit::AppContext as _;
        return cx.new(|_| worker::Backdropped(view)).into();
    }
    let _ = cx;
    view
}

/// The window's own view back out of [`wrap`].
pub(crate) fn content(view: &AnyView, cx: &App) -> AnyView {
    #[cfg(target_os = "linux")]
    if let Ok(wrapper) = view.clone().downcast::<worker::Backdropped>() {
        return wrapper.read(cx).0.clone();
    }
    let _ = cx;
    view.clone()
}
