//! The Viewport page: rendering, overlays, the camera, and the renderer's
//! calibration constants behind a disclosure.

use std::rc::Rc;

use gpui_kit::component::h_flex;
use gpui_kit::component::slider::{SliderEvent, SliderState, SliderValue};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_viewer::{CameraFeel, QualityLevel};

use crate::pacing::UnfocusedFps;
use crate::settings::FEEL_SCALE_RANGE;
use crate::tokens;

use super::super::Shell;
use super::kit::{
    self, mono, readout, segmented, slider, soon_pill, text, toggle, OnPick, Row, Section,
};
use super::SettingsWindow;

/// Where Manual starts when the mode was Automatic.
const MANUAL_START: u8 = 10;

/// The page's four sliders. Each is moved by the pointer here and by
/// anything else that changes its setting (a reset, the dock), so each
/// render puts the setting's value back on it.
pub(super) struct Sliders {
    quality: Entity<SliderState>,
    sensitivity: Entity<SliderState>,
    speed: Entity<SliderState>,
    smoothing: Entity<SliderState>,
}

/// One camera feel value: which field of [`CameraFeel`] a slider moves.
type Field = fn(&mut CameraFeel) -> &mut f32;

impl Sliders {
    pub(super) fn new(
        shell: &Entity<Shell>,
        cx: &mut Context<SettingsWindow>,
    ) -> (Self, Vec<Subscription>) {
        let (mode, feel) = {
            let shell = shell.read(cx);
            (shell.quality_choice, shell.camera_feel)
        };
        let level = match mode {
            QualityLevel::Level(level) => level,
            QualityLevel::Automatic => MANUAL_START,
        };
        let rail = |cx: &mut Context<SettingsWindow>, min: f32, max: f32, step: f32, value: f32| {
            cx.new(|_| {
                SliderState::new()
                    .min(min)
                    .max(max)
                    .step(step)
                    .default_value(value)
            })
        };
        let (low, high) = FEEL_SCALE_RANGE;
        let sliders = Sliders {
            quality: rail(
                cx,
                f32::from(QualityLevel::MIN),
                f32::from(QualityLevel::MAX),
                1.,
                f32::from(level),
            ),
            sensitivity: rail(cx, low, high, 0.1, feel.sensitivity),
            speed: rail(cx, low, high, 0.1, feel.speed),
            smoothing: rail(cx, 0., 1., 0.05, feel.smoothing),
        };
        let mut subscriptions =
            vec![
                cx.subscribe(&sliders.quality, |this, _, event: &SliderEvent, cx| {
                    if let SliderEvent::Change(SliderValue::Single(value)) = event {
                        let mode = QualityLevel::Level(value.round() as u8);
                        this.shell
                            .update(cx, |shell, cx| shell.set_quality(mode, cx));
                    }
                }),
            ];
        let feel_fields: [(&Entity<SliderState>, Field); 3] = [
            (&sliders.sensitivity, |feel| &mut feel.sensitivity),
            (&sliders.speed, |feel| &mut feel.speed),
            (&sliders.smoothing, |feel| &mut feel.smoothing),
        ];
        for (state, field) in feel_fields {
            subscriptions.push(
                cx.subscribe(state, move |this, _, event: &SliderEvent, cx| {
                    if let SliderEvent::Change(SliderValue::Single(value)) = event {
                        this.shell.update(cx, |shell, cx| {
                            let mut feel = shell.camera_feel;
                            // Rounded to the slider's own step, so a drag lands
                            // on the value the readout shows.
                            *field(&mut feel) = (value * 100.).round() / 100.;
                            shell.set_camera_feel(feel, cx);
                        });
                    }
                }),
            );
        }
        (sliders, subscriptions)
    }

    /// Puts each setting's current value back on its slider.
    fn sync(&self, mode: QualityLevel, feel: CameraFeel, window: &mut Window, cx: &mut App) {
        let mut put = |state: &Entity<SliderState>, value: f32| {
            if (state.read(cx).value().end() - value).abs() > 1e-4 {
                state.update(cx, |state, cx| state.set_value(value, window, cx));
            }
        };
        if let QualityLevel::Level(level) = mode {
            put(&self.quality, f32::from(level));
        }
        put(&self.sensitivity, feel.sensitivity);
        put(&self.speed, feel.speed);
        put(&self.smoothing, feel.smoothing);
    }
}

impl SettingsWindow {
    pub(super) fn viewport(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<Section> {
        let (mode, fps, axis, occluded, guides, ortho, feel) = {
            let shell = self.shell.read(cx);
            (
                shell.quality_choice,
                shell.unfocused_fps,
                shell.axis_indicator,
                shell.selection_occluded,
                shell.light_guides_shown(),
                shell.orthographic,
                shell.camera_feel,
            )
        };
        self.sliders.sync(mode, feel, window, cx);
        let defaults = CameraFeel::default();

        let manual_level = self.sliders.quality.read(cx).value().end().round() as u8;
        let quality = h_flex()
            .gap(px(8.))
            .items_center()
            .child(segmented(
                "quality-mode",
                24.,
                vec![
                    (
                        "Automatic",
                        mode == QualityLevel::Automatic,
                        self.shell_fn(|shell, cx| shell.set_quality(QualityLevel::Automatic, cx)),
                    ),
                    (
                        "Manual",
                        mode != QualityLevel::Automatic,
                        self.shell_fn(move |shell, cx| {
                            shell.set_quality(QualityLevel::Level(manual_level), cx)
                        }),
                    ),
                ],
            ))
            .when(mode != QualityLevel::Automatic, |this| {
                this.child(
                    h_flex()
                        .gap(px(10.))
                        .items_center()
                        .child(slider(&self.sliders.quality, cx))
                        .child(readout(manual_level.to_string())),
                )
            });
        let fps_control = segmented(
            "unfocused-fps",
            26.,
            [UnfocusedFps::Fps25, UnfocusedFps::Fps30]
                .into_iter()
                .map(|preset| {
                    let label = match preset {
                        UnfocusedFps::Fps25 => "25 fps",
                        UnfocusedFps::Fps30 => "30 fps",
                    };
                    (
                        label,
                        fps == preset,
                        self.shell_fn(move |shell, cx| shell.set_unfocused_fps(preset, cx)),
                    )
                })
                .collect(),
        );
        let rendering = Section::new(
            "Rendering",
            vec![
                Row::new("Graphics quality", quality)
                    .describe("Automatic follows the frame rate. Manual pins a level from 1 to 21.")
                    .changed(mode != QualityLevel::Automatic, |shell, cx| {
                        shell.set_quality(QualityLevel::Automatic, cx)
                    }),
                Row::new("Frame rate when unfocused", fps_control)
                    .describe("Caps the viewport while another window has focus.")
                    .changed(fps != UnfocusedFps::DEFAULT, |shell, cx| {
                        shell.set_unfocused_fps(UnfocusedFps::DEFAULT, cx)
                    }),
            ],
        );

        let overlays = Section::new(
            "Overlays",
            vec![
                Row::new(
                    "Orientation indicator",
                    toggle(
                        "axis",
                        axis,
                        self.set(move |shell, cx| shell.set_axis_indicator(!axis, cx)),
                    ),
                )
                .describe("The axis gizmo in the viewport\u{2019}s corner.")
                .changed(!axis, |shell, cx| shell.set_axis_indicator(true, cx)),
                // On draws through: the row is named for the outline showing
                // behind geometry, the setting for it being hidden there.
                Row::new(
                    "Selection box behind geometry",
                    toggle(
                        "through",
                        !occluded,
                        self.set(move |shell, cx| shell.set_selection_occluded(!occluded, cx)),
                    ),
                )
                .describe("Draw the selection outline through parts in front of it.")
                .changed(occluded, |shell, cx| {
                    shell.set_selection_occluded(false, cx)
                }),
                Row::new(
                    "Light guides",
                    toggle(
                        "guides",
                        guides,
                        self.set(|shell, cx| shell.toggle_light_guides(cx)),
                    ),
                )
                .describe("Range and cone of the selected light.")
                .changed(!guides, |shell, cx| shell.toggle_light_guides(cx)),
            ],
        );

        let feel_row =
            |label, description, state: &Entity<SliderState>, shown: String, field: Field| {
                let changed = {
                    let (mut now, mut default) = (feel, defaults);
                    (*field(&mut now) - *field(&mut default)).abs() > 1e-4
                };
                Row::new(
                    label,
                    h_flex()
                        .gap(px(10.))
                        .items_center()
                        .child(slider(state, cx))
                        .child(readout(shown)),
                )
                .describe(description)
                .changed(changed, move |shell, cx| {
                    let mut reset = shell.camera_feel;
                    *field(&mut reset) = *field(&mut CameraFeel::default());
                    shell.set_camera_feel(reset, cx);
                })
            };
        let camera = Section::new(
            "Camera",
            vec![
                Row::new(
                    "Orthographic camera",
                    toggle(
                        "ortho",
                        ortho,
                        self.set(move |shell, cx| shell.set_orthographic(!ortho, cx)),
                    ),
                )
                .describe("No perspective. Handy for lining parts up.")
                .changed(ortho, |shell, cx| shell.set_orthographic(false, cx)),
                feel_row(
                    "Mouse sensitivity",
                    "Free camera look speed.",
                    &self.sliders.sensitivity,
                    format!("{:.1}\u{d7}", feel.sensitivity),
                    |feel| &mut feel.sensitivity,
                ),
                feel_row(
                    "Camera speed",
                    "WASD movement speed.",
                    &self.sliders.speed,
                    format!("{:.1}\u{d7}", feel.speed),
                    |feel| &mut feel.speed,
                ),
                feel_row(
                    "Smoothing",
                    "Ease camera moves in and out.",
                    &self.sliders.smoothing,
                    format!("{:.2}", feel.smoothing),
                    |feel| &mut feel.smoothing,
                ),
            ],
        );

        vec![rendering, overlays, camera, self.advanced(cx)]
    }

    /// Advanced › Renderer calibration: on the roadmap, so its fields show
    /// the values the renderer is built with and take no input.
    fn advanced(&self, cx: &mut Context<Self>) -> Section {
        let open = self.advanced_open;
        let disclosure = h_flex()
            .id("calibration")
            .w_full()
            .h(px(44.))
            .px(px(16.))
            .gap(px(10.))
            .items_center()
            .cursor_pointer()
            .hover(|this| this.bg(rgba(0xFFFFFF04)))
            .on_click(cx.listener(|this, _, _, cx| {
                this.advanced_open = !this.advanced_open;
                cx.notify();
            }))
            .child(div().text_color(tokens::text2()).child(kit::icon(
                if open {
                    "chevron-down"
                } else {
                    "chevron-right"
                },
                13.,
            )))
            .child(
                text(12.5, 17.)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(tokens::text2())
                    .child("Renderer calibration"),
            )
            .child(soon_pill("calibration-soon"))
            .child(div().flex_1())
            .child(text(11.5, 16.).text_color(tokens::text3()).child(
                "For matching Roblox\u{2019}s look. Leave these alone unless you know why.",
            ));
        // The constants as the renderer has them (`rbx_viewer::lighting`,
        // `renderer/atmosphere.wgsl`, `renderer/lighting.wgsl`,
        // `quality/table.rs`).
        let constants = [
            ("SUN_BASE", "0.45"),
            ("ATMOSPHERE_DENSITY_SCALE", "0.0013"),
            ("PLASTIC_SPEC_STRENGTH", "0.28"),
            ("Quality bands", "8 bands"),
        ];
        let rows = if open {
            constants
                .into_iter()
                .map(|(name, value)| Row::new(name, field(value)).mono().inert())
                .collect()
        } else {
            Vec::new()
        };
        Section {
            head: Some(disclosure.into_any_element()),
            ..Section::new("Advanced", rows)
        }
    }

    /// Like [`SettingsWindow::set`], for a control that isn't clicked
    /// through a `ClickEvent` of its own (a segment).
    fn shell_fn(&self, f: impl Fn(&mut Shell, &mut Context<Shell>) + 'static) -> OnPick {
        let shell = self.shell.clone();
        Rc::new(move |_, cx| shell.update(cx, |shell, cx| f(shell, cx)))
    }
}

/// A 110×30 mono number field.
fn field(value: &'static str) -> Div {
    h_flex()
        .w(px(110.))
        .h(px(30.))
        .px(px(10.))
        .items_center()
        .justify_end()
        .border_1()
        .border_color(tokens::border2())
        .rounded(px(6.))
        .bg(tokens::dock())
        .child(mono(11.5, 16.).text_color(tokens::text()).child(value))
}
