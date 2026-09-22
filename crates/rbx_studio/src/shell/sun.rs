//! The Sun tool's writes and its ribbon group. The gestures themselves are
//! `crate::sun`'s.
//!
//! A step lands in `Lighting` the way a transform drag lands in a part: each
//! one written and reflected straight away, so the sky and the shadows follow
//! the cursor, and one undo step for the whole gesture, opened by its first
//! real write (see `shell::drag::write_drag`).

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::Ref;
use rbx_viewer::pick::{self, PartSurface, Ray};
use rbx_viewer::sun::{place, Body};

use crate::sun::{self, Mode};
use crate::tokens;
use crate::transform::{Action, Tool};

use super::ribbon;
use super::Shell;

/// Why the whole group is greyed in a file with nothing for it to write to.
const NO_LIGHTING: &str = "this place has no Lighting service";

impl Shell {
    /// One step of the Sun tool's gesture — see `ViewportAction::Sun`.
    pub(super) fn sun_step(&mut self, ray: Ray, first: bool, cx: &mut Context<Self>) {
        let Some(lighting) = sun::lighting(&self.dom) else {
            return;
        };
        let meshes = self.viewport.read(cx).meshes().clone();
        // The nearest part's drawn surface, not the box around it: Face and
        // Glint aim off a wedge's slope or a ball's curve as it looks.
        let hit = |ray: Ray, exclude: Option<Ref>| {
            pick::parts_along(&self.dom, &self.database, &meshes, ray)
                .into_iter()
                .filter(|&part| Some(part) != exclude)
                .find_map(|part| {
                    let surface = PartSurface::read(&self.dom, &self.database, &meshes, part)?;
                    let (distance, normal) = surface.raycast(ray)?;
                    let point = ray.at(distance);
                    Some((part, sun::Surface { point, normal }))
                })
        };
        if first {
            self.sun.press(ray, hit);
        }
        let Some(aim) = self.sun.aim(ray, first, hit) else {
            return;
        };

        let placement = place(self.sun.body, aim.toward);
        let writes = self
            .dom
            .get(lighting)
            .map(|service| sun::writes(service.properties(), &placement))
            .unwrap_or_default();
        if !writes.is_empty() {
            if self.sun.opens_step() {
                self.push_history();
            }
            for (name, value) in writes {
                if let Err(err) = self.dom.set_property(lighting, name, value) {
                    self.output.push_warning(&format!("sun: {err}"));
                }
            }
            // Overwrites the entry's log, as a transform drag's steps do:
            // every step writes the same one service.
            let changes = self.dom.take_changes();
            self.reflect_changes(&changes, cx);
            self.record_history_change(changes);
        }

        let guide = aim.guide(&placement);
        let text = SharedString::from(sun::readout(self.sun.body, &placement));
        self.viewport
            .update(cx, |viewport, cx| viewport.show_sun(guide, text, cx));
        cx.notify();
    }

    /// Enters the Sun tool, placing `body`, or keeps it with a new `mode`.
    fn use_sun(&mut self, body: Option<Body>, mode: Option<Mode>, cx: &mut Context<Self>) {
        if let Some(body) = body {
            self.sun.body = body;
        }
        if let Some(mode) = mode {
            self.sun.mode = mode;
        }
        self.transform_action(Action::Use(Tool::Sun), cx);
    }

    /// A tile each for the sun and the moon — either one enters the tool,
    /// placing that body — and the four gestures beside them. Picking a
    /// gesture enters the tool too: nobody picks one to leave it unused.
    pub(super) fn sun_tiles(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let nav = &self.ribbon_nav;
        let bodies = [
            (Body::Sun, "ribbon-sun", IconName::Sun, "Sun"),
            (Body::Moon, "ribbon-moon", IconName::Moon, "Moon"),
        ];
        let rows = |pair: [Mode; 2]| pair.map(|mode| (mode, mode_id(mode), mode_icon(mode)));

        if sun::lighting(&self.dom).is_none() {
            let mut group: Vec<AnyElement> = bodies
                .into_iter()
                .map(|(_, id, icon, label)| {
                    ribbon::unavailable_tile(id, icon, label, NO_LIGHTING).into_any_element()
                })
                .collect();
            for pair in [[Mode::Sky, Mode::Face], [Mode::Shadow, Mode::Glint]] {
                let rows = rows(pair).map(|(mode, id, icon)| {
                    ribbon::unavailable_row(id, icon, mode.label(), NO_LIGHTING)
                });
                group.push(ribbon::stack(rows.into()).into_any_element());
            }
            return group;
        }

        let active = self.transform.tool == Tool::Sun;
        let current = self.sun;
        let mut group: Vec<AnyElement> = bodies
            .into_iter()
            .map(|(body, id, icon, label)| {
                ribbon::tile(nav, id, icon, label, cx)
                    .when(active && current.body == body, |this| {
                        ribbon::selected(this, tokens::tool_sun())
                    })
                    .tooltip(move |window, cx| {
                        super::tooltip::text(
                            format!("{label} — place it by pointing at the scene"),
                            window,
                            cx,
                        )
                    })
                    .on_click(cx.listener(move |shell, _, _, cx| {
                        shell.use_sun(Some(body), None, cx);
                    }))
                    .into_any_element()
            })
            .collect();
        for pair in [[Mode::Sky, Mode::Face], [Mode::Shadow, Mode::Glint]] {
            let rows = rows(pair).map(|(mode, id, icon)| {
                // Marked only while the tool is in use: a remembered choice
                // on an idle tool would spend the accent on nothing active.
                ribbon::live_stack_row(nav, id, icon, mode.label(), cx)
                    .when(active && current.mode == mode, |this| {
                        ribbon::selected(this, tokens::tool_sun())
                    })
                    .tooltip(move |window, cx| {
                        super::tooltip::text(
                            format!("{} — {}", mode.label(), mode.hint()),
                            window,
                            cx,
                        )
                    })
                    .on_click(cx.listener(move |shell, _, _, cx| {
                        shell.use_sun(None, Some(mode), cx);
                    }))
            });
            group.push(ribbon::stack(rows.into()).into_any_element());
        }
        group
    }
}

fn mode_id(mode: Mode) -> &'static str {
    match mode {
        Mode::Sky => "ribbon-sun-sky",
        Mode::Face => "ribbon-sun-face",
        Mode::Shadow => "ribbon-sun-shadow",
        Mode::Glint => "ribbon-sun-glint",
    }
}

fn mode_icon(mode: Mode) -> IconName {
    match mode {
        Mode::Sky => IconName::CloudSun,
        Mode::Face => IconName::Spotlight,
        Mode::Shadow => IconName::Contrast,
        Mode::Glint => IconName::Sparkle,
    }
}
