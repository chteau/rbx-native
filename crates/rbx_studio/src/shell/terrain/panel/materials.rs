//! The material picker: a tile per terrain material, swatched in the
//! colour the place draws it with (`Terrain.MaterialColors`, or
//! `WaterColor` for water), so what is picked is what will appear.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::Variant;
use rbx_terrain::{Material, MaterialColors};

use crate::terrain::{find_terrain, MaterialChoice};
use crate::tokens;

use super::super::super::Shell;

impl Shell {
    /// `with_air` adds Air (Fill's "replace with nothing" and "fill the
    /// empty space"); water is offered everywhere but Paint, which keeps
    /// shape and so has no water to give.
    pub(super) fn material_picker(
        &self,
        title: &'static str,
        choice: MaterialChoice,
        with_air: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (colors, water) = self.terrain_colors();
        let current = self.terrain.settings.material(choice);
        let paint = matches!(
            choice,
            MaterialChoice::PaintSource | MaterialChoice::PaintTarget
        );
        let materials = Material::ALL.into_iter().filter(|m| match m {
            Material::Air => with_air,
            Material::Water => !paint,
            _ => true,
        });
        v_flex()
            .gap(px(4.))
            .child(
                h_flex()
                    .justify_between()
                    .child(div().text_color(tokens::text2()).child(title))
                    .child(
                        div()
                            .text_color(tokens::text3())
                            .text_size(tokens::text_sm())
                            .child(current.name()),
                    ),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap(px(4.))
                    .children(materials.map(|material| {
                        let on = material == current;
                        let swatch = match material {
                            Material::Water => water,
                            Material::Air => [0, 0, 0],
                            other => colors.get(other),
                        };
                        div()
                            .id(SharedString::from(format!(
                                "terrain-material-{title}-{}",
                                material.name()
                            )))
                            .size(px(30.))
                            .rounded(tokens::radius_tiny())
                            .cursor_pointer()
                            .border_2()
                            .border_color(if on {
                                tokens::accent_line()
                            } else {
                                tokens::border()
                            })
                            .when(material == Material::Air, |this| {
                                this.bg(tokens::field_select())
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(tokens::text_xxs())
                                    .text_color(tokens::text3())
                                    .child("Air")
                            })
                            .when(material != Material::Air, |this| {
                                this.bg(rgb(u32::from_be_bytes([
                                    0, swatch[0], swatch[1], swatch[2],
                                ])))
                            })
                            .hover(|this| this.border_color(tokens::accent_hover()))
                            .focus_visible(|this| this.shadow(tokens::focus_ring(tokens::dock())))
                            .tooltip(move |window, cx| {
                                super::super::super::tooltip::text(material.name(), window, cx)
                            })
                            .on_click(cx.listener(move |shell, _, _, cx| {
                                shell.terrain.settings.set_material(choice, material);
                                // One pick is the whole of the Alt-click
                                // picker's job.
                                shell.terrain.picker_at = None;
                                cx.notify();
                            }))
                    })),
            )
            .into_any_element()
    }

    /// The `Alt`-click picker: the active brush's material swatches at the
    /// cursor, closed by a pick or a click elsewhere.
    pub(in crate::shell) fn terrain_picker_popup(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let at = self.terrain.picker_at?;
        let choice = match self.terrain.tool {
            Some(crate::terrain::TerrainTool::Paint) => MaterialChoice::PaintTarget,
            _ => MaterialChoice::Brush,
        };
        let grid = self.material_picker("Material", choice, false, cx);
        let surface = super::super::super::menu::surface()
            .id("terrain-material-picker")
            .w(px(260.))
            .p(px(10.))
            .occlude()
            .relative()
            .on_mouse_down_out(cx.listener(|shell, _: &MouseDownEvent, _, cx| {
                shell.terrain.picker_at = None;
                cx.notify();
            }))
            .child(grid)
            .child(gpui_base::occlusion::probe());
        Some(
            deferred(
                anchored()
                    .position(at)
                    .snap_to_window_with_margin(px(8.))
                    .child(surface),
            )
            .into_any_element(),
        )
    }

    /// The place's terrain colours and its water colour, as sRGB bytes.
    fn terrain_colors(&self) -> (MaterialColors, [u8; 3]) {
        let Some(instance) = find_terrain(&self.dom).and_then(|r| self.dom.get(r)) else {
            return (MaterialColors::default(), [12, 84, 92]);
        };
        let props = instance.properties();
        let colors = match props.get("MaterialColors") {
            Some(Variant::String(text)) => MaterialColors::decode(text.as_bytes()),
            Some(Variant::Unknown { raw, .. }) => MaterialColors::decode(raw),
            _ => None,
        }
        .unwrap_or_default();
        let water = match props.get("WaterColor") {
            Some(Variant::Color3(c)) => {
                [c.r, c.g, c.b].map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
            }
            _ => [12, 84, 92],
        };
        (colors, water)
    }
}
