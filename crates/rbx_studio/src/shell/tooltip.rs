//! §6, the hover label an icon-only button needs to be more than a
//! pictogram.
//!
//! A thin wrapper over the toolkit's own tooltip rather than a hand-rolled
//! overlay: hover timing, anchoring, flipping when there's no room above,
//! and layering above everything else in the window are all already solved
//! there, and none of them are what this editor has anything new to say
//! about. What that costs is the spec's 4px triangle pointer, which the
//! stock tooltip doesn't draw, and control over the exact hover delay.
//!
//! The text doubles as the accessible label for the control it belongs to
//! (§7.3), which is why every icon-only button in the shell has one.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::*;

pub(crate) fn text(label: impl Into<SharedString>, window: &mut Window, cx: &mut App) -> AnyView {
    Tooltip::new(label.into()).build(window, cx)
}

/// Re-checks hover after a wheel scroll, for whatever window this is a
/// child of.
///
/// GPUI only re-evaluates hover, highlights and tooltips both, when the
/// pointer moves, and a wheel scroll moves the content instead. It does drop
/// a tooltip that is already showing on a scroll, but not one still waiting
/// out its delay: that one opens afterwards with the old row's name, over
/// whatever row has scrolled under the pointer, and stays there, because its
/// "still hovered?" check compares the pointer to the bounds the old row had
/// when the hover began. So two frames on (one to draw the scroll, one to
/// hit-test the result) this replays the pointer where it already is, and
/// GPUI sorts out hover from there exactly as if the mouse had moved.
///
/// A touchpad or kinetic scroll sends dozens of wheel events a second, so
/// they coalesce: each one bumps a per-window counter, and only the replay
/// queued by the latest one still matches it when its two frames are up.
///
/// The replayed move has no button pressed whatever the real state is. The
/// viewport is the one place that notices: a wheel zoom in the middle of a
/// part or handle drag records the move as the drag's next step, which
/// re-raycasts under the new camera so the part stays under the cursor, as
/// it does in Studio.
///
/// A zero-size element so it can sit in the title bar every window draws
/// first: its capture-phase listener then runs before any scroll area that
/// stops the wheel in that phase.
pub(crate) fn rehover_on_scroll() -> impl IntoElement {
    canvas(
        |_, _, _| {},
        |_, _, window, _| {
            let latest = window.with_global_id("rehover-on-scroll".into(), |id, window| {
                window.with_element_state(id, |latest: Option<Rc<Cell<u64>>>, _| {
                    let latest = latest.unwrap_or_default();
                    (latest.clone(), latest)
                })
            });
            window.on_mouse_event(move |_: &ScrollWheelEvent, phase, window, _| {
                if !phase.capture() {
                    return;
                }
                let this = latest.get().wrapping_add(1);
                latest.set(this);
                let latest = latest.clone();
                replay_pointer(window, move || latest.get() == this);
            })
        },
    )
    .absolute()
    .size_0()
}

/// Re-checks hover after code scrolled a list under a still pointer, the
/// way [`rehover_on_scroll`] does after a wheel: the Explorer's arrow keys,
/// Home, End and type-ahead move its rows with `scroll_to_item`, and without
/// this the row that lands under the pointer never gets its hover.
pub(crate) fn rehover(window: &mut Window) {
    replay_pointer(window, || true);
}

/// Two frames on (one to draw the scroll, one to hit-test the result),
/// replays the pointer where it already is, if `still_wanted` says so.
fn replay_pointer(window: &mut Window, still_wanted: impl FnOnce() -> bool + 'static) {
    window.on_next_frame(move |window, _| {
        window.on_next_frame(move |window, cx| {
            if !still_wanted() {
                return;
            }
            let event = MouseMoveEvent {
                position: window.mouse_position(),
                pressed_button: None,
                modifiers: window.modifiers(),
            };
            window.dispatch_event(PlatformInput::MouseMove(event), cx);
        })
    });
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::time::Duration;

    use gpui_kit::prelude::*;
    use gpui_kit::{
        canvas, deferred, div, point, px, Context, IntoElement, Modifiers, Render, TestAppContext,
        Window,
    };

    /// A tooltip that counts the frames it was painted in.
    struct Counted(Rc<Cell<usize>>);

    impl Render for Counted {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let painted = self.0.clone();
            div().w(px(80.)).h(px(20.)).child(
                canvas(
                    |_, _, _| {},
                    move |_, _, _, _| painted.set(painted.get() + 1),
                )
                .size_full(),
            )
        }
    }

    /// A button with a tooltip and, with `menu`, the menu it opened just
    /// under it, in the layer menus are drawn in.
    struct Trigger {
        menu: bool,
        painted: Rc<Cell<usize>>,
    }

    impl Render for Trigger {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let painted = self.painted.clone();
            div()
                .size_full()
                .child(
                    div()
                        .id("trigger")
                        .absolute()
                        .left(px(10.))
                        .top(px(10.))
                        .size(px(20.))
                        .tooltip(move |_, cx| cx.new(|_| Counted(painted.clone())).into()),
                )
                .when(self.menu, |this| {
                    this.child(deferred(
                        div()
                            .id("menu")
                            .absolute()
                            .left(px(10.))
                            .top(px(34.))
                            .w(px(120.))
                            .h(px(80.))
                            .occlude(),
                    ))
                })
        }
    }

    /// Hovers the trigger past the tooltip's delay; how often its tooltip
    /// was painted.
    fn tooltip_frames(cx: &mut TestAppContext, menu: bool) -> usize {
        let painted = Rc::new(Cell::new(0));
        let (_view, cx) = cx.add_window_view({
            let painted = painted.clone();
            move |_, _| Trigger { menu, painted }
        });
        cx.run_until_parked();
        cx.simulate_mouse_move(point(px(20.), px(20.)), None, Modifiers::none());
        cx.executor().advance_clock(Duration::from_secs(2));
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        painted.get()
    }

    #[gpui_kit::test]
    fn a_hovered_control_shows_its_tooltip(cx: &mut TestAppContext) {
        assert!(tooltip_frames(cx, false) > 0);
    }

    /// The tooltip layer is drawn over every deferred one: a tooltip from
    /// under an open menu sat on top of it.
    #[gpui_kit::test]
    fn a_tooltip_never_covers_an_open_menu(cx: &mut TestAppContext) {
        assert_eq!(tooltip_frames(cx, true), 0);
    }
}
