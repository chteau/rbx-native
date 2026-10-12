//! The Viewport page: rendering, overlays, the camera, and the renderer's
//! calibration constants behind a disclosure.

use gpui_kit::component::h_flex;
use gpui_kit::component::input::InputState;
use gpui_kit::component::slider::{SliderEvent, SliderState, SliderValue};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_viewer::{CameraFeel, QualityLevel};

use crate::pacing::UnfocusedFps;
use crate::settings::FEEL_SCALE_RANGE;
use crate::tokens;

use super::super::toolbar::snap::NumberField;
use super::super::Shell;
use super::dragger::{committed, number};
use super::kit::{self, readout, segmented, slider, text, toggle, Row, Section};
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

        vec![rendering, overlays, camera, self.advanced(window, cx)]
    }

    /// Advanced › Renderer calibration: the renderer's three tuned constants
    /// as number fields (see `rbx_viewer::Calibration`), and the quality
    /// bands, which are still the renderer's own table.
    fn advanced(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Section {
        let open = self.advanced_open;
        let calibration = self.shell.read(cx).calibration();
        let disclosure = h_flex()
            .id("calibration")
            .w_full()
            .h(px(44.))
            .px(px(16.))
            .gap(px(10.))
            .items_center()
            .cursor_pointer()
            .hover(|this| this.bg(tokens::hover_faint()))
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
            .child(div().flex_1())
            .child(text(11.5, 16.).text_color(tokens::text3()).child(
                "For matching Roblox\u{2019}s look. Leave these alone unless you know why.",
            ));
        let tuned = rbx_viewer::Calibration::default();
        let rows = if open {
            let focused = |input: &Entity<InputState>, cx: &App| {
                input.read(cx).focus_handle(cx).is_focused(window)
            };
            let fields = &self.calibration;
            vec![
                Row::new(
                    "SUN_BASE",
                    number(
                        &fields.sun_base.input,
                        "",
                        focused(&fields.sun_base.input, cx),
                    ),
                )
                .mono()
                .describe("The sun lamp\u{2019}s strength at Brightness 1.")
                .changed(
                    calibration.sun_base != tuned.sun_base,
                    move |shell, cx| {
                        let mut calibration = shell.calibration();
                        calibration.sun_base = tuned.sun_base;
                        shell.set_calibration(calibration, cx);
                    },
                ),
                Row::new(
                    "ATMOSPHERE_DENSITY_SCALE",
                    number(
                        &fields.atmosphere.input,
                        "",
                        focused(&fields.atmosphere.input, cx),
                    ),
                )
                .mono()
                .describe("Haze per stud at Atmosphere Density 1.")
                .changed(
                    calibration.atmosphere_density_scale != tuned.atmosphere_density_scale,
                    move |shell, cx| {
                        let mut calibration = shell.calibration();
                        calibration.atmosphere_density_scale = tuned.atmosphere_density_scale;
                        shell.set_calibration(calibration, cx);
                    },
                ),
                Row::new(
                    "PLASTIC_SPEC_STRENGTH",
                    number(
                        &fields.plastic.input,
                        "",
                        focused(&fields.plastic.input, cx),
                    ),
                )
                .mono()
                .describe("Plastic\u{2019}s highlight; every textured material scales from it.")
                .changed(
                    calibration.plastic_spec_strength != tuned.plastic_spec_strength,
                    move |shell, cx| {
                        let mut calibration = shell.calibration();
                        calibration.plastic_spec_strength = tuned.plastic_spec_strength;
                        shell.set_calibration(calibration, cx);
                    },
                ),
                Row::new(
                    "Quality bands",
                    h_flex().gap(px(4.)).children(fields.bands.iter().map(|band| {
                        band_number(&band.input, focused(&band.input, cx))
                    })),
                )
                .describe(
                    "The highest level each of the first seven bands covers; the eighth runs to 21.",
                )
                .changed(
                    calibration.quality_bands != tuned.quality_bands,
                    move |shell, cx| {
                        let mut calibration = shell.calibration();
                        calibration.quality_bands = tuned.quality_bands;
                        shell.set_calibration(calibration, cx);
                    },
                ),
            ]
        } else {
            Vec::new()
        };
        Section {
            head: Some(disclosure.into_any_element()),
            ..Section::new("Advanced", rows)
        }
    }
}

/// One calibration value, by which field of [`rbx_viewer::Calibration`] it is.
type Value = fn(&mut rbx_viewer::Calibration) -> &mut f32;

/// Advanced's three calibration fields. Typed into as numbers, applied on
/// Enter or leaving the field when in range (see `settings::calibration_range`)
/// and put back otherwise, like settings.json refuses one.
pub(super) struct CalibrationFields {
    pub(super) sun_base: NumberField,
    pub(super) atmosphere: NumberField,
    pub(super) plastic: NumberField,
    pub(super) bands: [NumberField; 7],
}

impl CalibrationFields {
    pub(super) fn new(
        shell: &Entity<Shell>,
        window: &mut Window,
        cx: &mut Context<SettingsWindow>,
    ) -> (Self, Vec<Subscription>) {
        let current = shell.read(cx).calibration();
        let mut subscriptions = Vec::new();
        let mut field = |value: Value, cx: &mut Context<SettingsWindow>| {
            let mut seed = current;
            let shown = *value(&mut seed);
            let (field, typed) = NumberField::new(shown, window, cx, move |this, text, cx| {
                let mut tuned = rbx_viewer::Calibration::default();
                let range = crate::settings::calibration_range(*value(&mut tuned));
                if let Some(typed) = committed(text, range, false) {
                    this.shell.update(cx, |shell, cx| {
                        let mut calibration = shell.calibration();
                        *value(&mut calibration) = typed;
                        shell.set_calibration(calibration, cx);
                    });
                }
                *value(&mut this.shell.read(cx).calibration())
            });
            subscriptions.push(typed);
            field
        };
        let sun_base = field(|c| &mut c.sun_base, cx);
        let atmosphere = field(|c| &mut c.atmosphere_density_scale, cx);
        let plastic = field(|c| &mut c.plastic_spec_strength, cx);
        let bands = std::array::from_fn(|index| {
            let (field, typed) = NumberField::new(
                f32::from(current.quality_bands[index]),
                window,
                cx,
                move |this, text, cx| {
                    let bands = this.shell.read(cx).calibration().quality_bands;
                    let below = if index == 0 { 0 } else { bands[index - 1] };
                    let above = if index == 6 { 21 } else { bands[index + 1] };
                    let range = (f32::from(below) + 1., f32::from(above) - 1.);
                    if let Some(typed) = committed(text, range, false).filter(|v| v.fract() == 0.) {
                        this.shell.update(cx, |shell, cx| {
                            let mut calibration = shell.calibration();
                            calibration.quality_bands[index] = typed as u8;
                            shell.set_calibration(calibration, cx);
                        });
                    }
                    f32::from(this.shell.read(cx).calibration().quality_bands[index])
                },
            );
            subscriptions.push(typed);
            field
        });
        let fields = CalibrationFields {
            sun_base,
            atmosphere,
            plastic,
            bands,
        };
        (fields, subscriptions)
    }
}

/// A 40×30 mono integer field, one of the quality band ends.
fn band_number(input: &Entity<InputState>, focused: bool) -> impl IntoElement {
    h_flex()
        .w(px(40.))
        .h(px(30.))
        .px(px(6.))
        .items_center()
        .border_1()
        .border_color(if focused {
            tokens::accent_line()
        } else {
            tokens::border2()
        })
        .rounded(px(6.))
        .bg(tokens::dock())
        .font_family(tokens::FONT_FAMILY_MONO)
        .child(
            component::input::Input::new(input)
                .appearance(false)
                .w_full()
                .px_0()
                .text_right()
                .text_size(px(11.5))
                .line_height(px(16.))
                .text_color(tokens::text()),
        )
}
