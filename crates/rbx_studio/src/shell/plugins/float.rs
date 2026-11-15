//! Plugin dock widgets the layout has floating (`InitialDockState.Float`,
//! or torn out or floated from their dock) get a window of their own, the
//! same `shell::panel_window` a torn-out Explorer gets. What is particular
//! to a widget is how big that window opens, Studio's `FloatingXSize` x
//! `FloatingYSize` never below its minimum, and that the host keeps where
//! it was last left in the plugin's settings, so it opens there next
//! session too.

use gpui_kit::*;
use rbx_lua::plugin::WidgetKind;

use crate::shell::layout::PluginDock;
use crate::shell::Shell;

/// `Enum.InitialDockState.Float`.
pub(super) const FLOAT: u8 = 4;
/// What Studio's frame and title bar take off a floating widget: Studio
/// 0.743 gives a 300 x 200 `FloatingXSize`/`YSize` widget an
/// `AbsoluteSize` of 298 x 176. Our window brings its own chrome, so the
/// content keeps Studio's size and the window grows around it.
const FRAME: (f32, f32) = (2.0, 24.0);
/// Window size for a widget whose info gives neither a size nor a minimum.
const DEFAULT_SIZE: (f32, f32) = (300.0, 200.0);

/// The content size a floating widget opens at: a `FloatingXSize` x
/// `FloatingYSize` window (never below the minimum) less its frame.
fn content_size(float: (f32, f32), min: (f32, f32)) -> (f32, f32) {
    let pick = |f: f32, m: f32, fallback: f32| {
        if f > 0.0 {
            f.max(m)
        } else if m > 0.0 {
            m
        } else {
            fallback
        }
    };
    (
        pick(float.0, min.0, DEFAULT_SIZE.0) - FRAME.0,
        pick(float.1, min.1, DEFAULT_SIZE.1) - FRAME.1,
    )
}

impl Shell {
    /// A floating plugin dock's window: where it was last left, if
    /// anywhere, the size it opens at otherwise, and the least it may
    /// shrink to, each with `chrome` (the window's own bars and frame)
    /// around the widget's content.
    pub(in crate::shell) fn plugin_window_bounds(
        &self,
        dock: PluginDock,
        chrome: Size<Pixels>,
    ) -> (Option<Bounds<Pixels>>, Size<Pixels>, Size<Pixels>) {
        let id = self.dock_widget(dock);
        let (float, min) = id
            .and_then(|id| self.plugins.ui.widgets.iter().find(|w| w.id == id))
            .and_then(|w| match w.kind {
                WidgetKind::Dock {
                    float_size,
                    min_size,
                    ..
                } => Some((float_size, min_size)),
                _ => None,
            })
            .unwrap_or_default();
        let saved = id
            .and_then(|id| self.plugins.host.as_ref()?.widget_rect(id))
            .map(|[x, y, w, h]| Bounds::new(point(px(x), px(y)), size(px(w), px(h))));
        let around = |(w, h): (f32, f32)| size(px(w) + chrome.width, px(h) + chrome.height);
        let floor = ((min.0 - FRAME.0).max(40.0), (min.1 - FRAME.1).max(20.0));
        (saved, around(content_size(float, min)), around(floor))
    }

    /// Keeps where a floating plugin dock's window was left, for the next
    /// time it opens, this session or a later one.
    pub(in crate::shell) fn remember_plugin_window(
        &mut self,
        dock: PluginDock,
        bounds: Bounds<Pixels>,
    ) {
        let Some(id) = self.dock_widget(dock) else {
            return;
        };
        if let Some(host) = self.plugins.host.as_mut() {
            let (origin, size) = (bounds.origin, bounds.size);
            let rect = [origin.x, origin.y, size.width, size.height];
            host.set_widget_rect(id, rect.map(f32::from));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{content_size, DEFAULT_SIZE, FRAME};

    #[test]
    fn opens_at_the_floating_size_less_the_frame_like_studio() {
        // Studio 0.743: 300 x 200 gives AbsoluteSize 298 x 176, 400 x 250
        // gives 398 x 226.
        assert_eq!(content_size((300.0, 200.0), (150.0, 100.0)), (298.0, 176.0));
        assert_eq!(content_size((400.0, 250.0), (0.0, 0.0)), (398.0, 226.0));
        assert_eq!(content_size((180.0, 150.0), (200.0, 100.0)), (198.0, 126.0));
        let default = (DEFAULT_SIZE.0 - FRAME.0, DEFAULT_SIZE.1 - FRAME.1);
        assert_eq!(content_size((0.0, 0.0), (0.0, 0.0)), default);
    }
}
