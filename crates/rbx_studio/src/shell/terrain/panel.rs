//! The Terrain Editor dock: Create and Edit tabs, the tools of the tab
//! showing, and the settings of the tool in use — laid out after
//! `terrain-editor.md`'s own per-tool tables.

mod brush;
mod controls;
mod materials;

use crate::shell::chrome::ScrollbarY as _;
use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_terrain::generate::Biome;

use crate::terrain::{FillMode, MaterialChoice, Tab, TerrainTool};
use crate::tokens;

use super::super::layout::Panel;
use super::super::menu;
use super::super::{chrome, Shell};
use super::fields::{Number, Rail, TerrainFields};
use controls::{action_button, chips, number_row, rail_row, section, switch_row};

impl Shell {
    pub(in crate::shell) fn terrain_dock(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Option<AnyElement>, Option<AnyElement>) {
        let overflow = menu::dropdown(
            self,
            menu::MenuId::TerrainOverflow,
            chrome::dock_options_trigger(
                "terrain-overflow",
                IconName::Ellipsis,
                16.,
                "Terrain Editor options",
            ),
            self.move_items(Panel::TerrainEditor),
            cx,
        );
        if self.terrain_fields.is_none() {
            self.terrain_fields = Some(TerrainFields::new(self, window, cx));
        }
        if let Some(fields) = &self.terrain_fields {
            fields.sync(&self.terrain, window, cx);
        }

        let tab = self.terrain.tab;
        let tabs = chips(
            "terrain-tab",
            &[(Tab::Create, "Create"), (Tab::Edit, "Edit")],
            tab,
            cx,
            |shell, tab, cx| {
                shell.terrain.tab = tab;
                cx.notify();
            },
        );
        let tools = match tab {
            Tab::Create => TerrainTool::CREATE.to_vec(),
            Tab::Edit => TerrainTool::EDIT.to_vec(),
        };
        let current = self
            .terrain
            .tool
            .filter(|_| self.transform.tool == crate::transform::Tool::Terrain);
        let grid = h_flex()
            .flex_wrap()
            .gap(px(6.))
            .children(tools.into_iter().map(|tool| {
                let on = current == Some(tool);
                div()
                    .id(SharedString::from(format!("terrain-tool-{}", tool.label())))
                    .w(px(76.))
                    .h(px(58.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(4.))
                    .rounded(tokens::radius_tile())
                    .cursor_pointer()
                    .text_size(tokens::text_xs())
                    .line_height(tokens::line_xs())
                    .text_color(if on { tokens::text() } else { tokens::text2() })
                    .bg(if on {
                        tokens::accent_soft()
                    } else {
                        tokens::tile()
                    })
                    .when(on, |this| {
                        this.border_1().border_color(tokens::accent_line())
                    })
                    .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
                    .focus_visible(|this| this.shadow(tokens::focus_ring(tokens::dock())))
                    .tooltip(move |window, cx| super::super::tooltip::text(tool.hint(), window, cx))
                    .on_click(cx.listener(move |shell, _, _, cx| shell.use_terrain_tool(tool, cx)))
                    .child(Icon::new(tool.icon()).size(px(20.)))
                    .child(tool.label())
            }));

        let mut sections: Vec<AnyElement> = Vec::new();
        if let Some(tool) = current {
            sections.extend(self.tool_sections(tool, cx));
        } else {
            sections.push(
                div()
                    .text_color(tokens::text3())
                    .text_size(tokens::text_sm())
                    .child("Pick a tool to start editing terrain.")
                    .into_any_element(),
            );
        }

        let body = div()
            .id("terrain-dock")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.terrain_scroll)
            .child(
                v_flex()
                    .p(px(14.))
                    .gap(px(14.))
                    .text_size(tokens::text_md())
                    .line_height(tokens::line_md())
                    .child(tabs)
                    .child(grid)
                    .children(sections),
            )
            .scrollbar_y(&self.terrain_scroll);
        (
            Some(overflow.into_any_element()),
            Some(body.into_any_element()),
        )
    }

    fn tool_sections(&mut self, tool: TerrainTool, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut out = Vec::new();
        if tool.uses_region() {
            out.push(self.region_section(tool, cx));
        }
        match tool {
            TerrainTool::Import => out.push(self.import_section(cx)),
            TerrainTool::Generate => out.push(self.generate_section(cx)),
            TerrainTool::Clear => out.push(
                section(
                    "CLEAR",
                    vec![
                        div()
                            .text_color(tokens::text2())
                            .text_size(tokens::text_sm())
                            .child("Removes every voxel of terrain in the place. Undo brings it back.")
                            .into_any_element(),
                        action_button("terrain-clear", "Clear", true, cx, |shell, cx| shell.terrain_clear(cx)),
                    ],
                )
                .into_any_element(),
            ),
            TerrainTool::Transform => out.push(self.transform_section(cx)),
            TerrainTool::Fill => out.push(self.terrain_fill_section(cx)),
            TerrainTool::SeaLevel => out.push(
                section(
                    "SEA LEVEL",
                    vec![h_flex()
                        .gap(px(8.))
                        .child(action_button("terrain-evaporate", "Evaporate", false, cx, |shell, cx| {
                            shell.terrain_sea_level(false, cx)
                        }))
                        .child(action_button("terrain-sea-create", "Create", true, cx, |shell, cx| {
                            shell.terrain_sea_level(true, cx)
                        }))
                        .into_any_element()],
                )
                .into_any_element(),
            ),
            TerrainTool::Select => out.push(
                section(
                    "SHORTCUTS",
                    vec![div()
                        .text_color(tokens::text3())
                        .text_size(tokens::text_sm())
                        .child(
                            "Drag across the terrain to draw a region. Ctrl+C copies it, Ctrl+X cuts, \
                             Ctrl+V pastes, Ctrl+D duplicates, Delete removes it. Shift on a ball \
                             scales evenly; Ctrl grows both sides.",
                        )
                        .into_any_element()],
                )
                .into_any_element(),
            ),
            _ if tool.is_brush() => {
                out.push(self.brush_section(tool, cx));
                out.push(self.brush_material_section(tool, cx));
            }
            _ => {}
        }
        out
    }

    fn fields(&self) -> &TerrainFields {
        self.terrain_fields
            .as_ref()
            .expect("built before the sections")
    }

    fn region_section(&mut self, tool: TerrainTool, cx: &mut Context<Self>) -> AnyElement {
        let fields = self.fields();
        let xyz =
            |make: fn(usize) -> Number| [0, 1, 2].map(|axis| fields.number(make(axis)).clone());
        let title = if tool == TerrainTool::Transform {
            "TRANSFORM"
        } else {
            "SELECTION"
        };
        let snap = self.terrain.settings.region_snap;
        section(
            title,
            vec![
                number_row("Position", xyz(Number::Position).to_vec(), cx),
                number_row("Size", xyz(Number::Size).to_vec(), cx),
                switch_row(
                    "terrain-snap",
                    "Snap to Voxels",
                    snap,
                    cx,
                    |shell, on, cx| {
                        shell.terrain.settings.region_snap = on;
                        cx.notify();
                    },
                ),
            ],
        )
        .into_any_element()
    }

    fn transform_section(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let fields = self.fields();
        let rotation = [0, 1, 2].map(|axis| fields.number(Number::Rotation(axis)).clone());
        let settings = &self.terrain.settings;
        let (merge, live) = (settings.merge_empty, settings.live_edit);
        let mut rows = vec![
            number_row("Rotation", rotation.to_vec(), cx),
            switch_row(
                "terrain-merge",
                "Merge Empty",
                merge,
                cx,
                |shell, on, cx| {
                    shell.terrain.settings.merge_empty = on;
                    cx.notify();
                },
            ),
            switch_row("terrain-live", "Live Edit", live, cx, |shell, on, cx| {
                shell.terrain.settings.live_edit = on;
                cx.notify();
            }),
        ];
        if !live {
            rows.push(action_button(
                "terrain-apply-transform",
                "Apply",
                true,
                cx,
                |shell, cx| shell.terrain_apply_transform(cx),
            ));
        }
        section("OPTIONS", rows).into_any_element()
    }

    fn terrain_fill_section(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let mode = self.terrain.settings.fill_mode;
        let mut rows = vec![chips(
            "terrain-fill-mode",
            &[(FillMode::Fill, "Fill"), (FillMode::Replace, "Replace")],
            mode,
            cx,
            |shell, mode, cx| {
                shell.terrain.settings.fill_mode = mode;
                cx.notify();
            },
        )];
        let label = if mode == FillMode::Fill {
            "Fill material"
        } else {
            "Replace"
        };
        rows.push(self.material_picker(label, MaterialChoice::FillSource, true, cx));
        if mode == FillMode::Replace {
            rows.push(self.material_picker("With", MaterialChoice::FillTarget, true, cx));
        }
        rows.push(action_button(
            "terrain-fill-apply",
            "Apply",
            true,
            cx,
            |shell, cx| shell.terrain_fill(cx),
        ));
        section("MATERIAL", rows).into_any_element()
    }

    fn import_section(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let name = |path: &Option<std::path::PathBuf>| {
            path.as_ref()
                .and_then(|p| p.file_name())
                .map_or("None".to_string(), |n| n.to_string_lossy().into_owned())
        };
        let (height, color) = (name(&self.terrain.heightmap), name(&self.terrain.colormap));
        let has_colormap = self.terrain.colormap.is_some();
        let mut rows = vec![
            controls::file_row("Heightmap", height, "terrain-heightmap", cx, |shell, cx| {
                shell.terrain_choose_image(false, cx)
            }),
            controls::file_row("Colormap", color, "terrain-colormap", cx, |shell, cx| {
                shell.terrain_choose_image(true, cx)
            }),
        ];
        if has_colormap {
            rows.push(action_button(
                "terrain-colormap-clear",
                "Remove colormap",
                false,
                cx,
                |shell, cx| {
                    shell.terrain.colormap = None;
                    cx.notify();
                },
            ));
        } else {
            rows.push(self.material_picker(
                "Default Material",
                MaterialChoice::ImportDefault,
                false,
                cx,
            ));
        }
        rows.push(action_button(
            "terrain-import",
            "Import",
            true,
            cx,
            |shell, cx| shell.terrain_import(cx),
        ));
        section("HEIGHTMAP", rows).into_any_element()
    }

    fn generate_section(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let fields = self.fields();
        let (blending, biome_size, seed) = (
            fields.rail(Rail::Blending).clone(),
            fields.rail(Rail::BiomeSize).clone(),
            fields.number(Number::Seed).clone(),
        );
        let settings = &self.terrain.settings.generate;
        let caves = settings.caves;
        let mut rows: Vec<AnyElement> = Biome::ALL
            .into_iter()
            .map(|biome| {
                let on = settings.biomes.contains(&biome);
                switch_row(
                    SharedString::from(format!("terrain-biome-{}", biome.label())),
                    biome.label(),
                    on,
                    cx,
                    move |shell, on, cx| {
                        let biomes = &mut shell.terrain.settings.generate.biomes;
                        biomes.retain(|b| *b != biome);
                        if on {
                            biomes.push(biome);
                        }
                        cx.notify();
                    },
                )
            })
            .collect();
        rows.push(rail_row("Blending", &blending, cx));
        rows.push(switch_row(
            "terrain-caves",
            "Caves",
            caves,
            cx,
            |shell, on, cx| {
                shell.terrain.settings.generate.caves = on;
                cx.notify();
            },
        ));
        rows.push(rail_row("Biome Size", &biome_size, cx));
        rows.push(number_row("Seed", vec![seed], cx));
        rows.push(action_button(
            "terrain-new-seed",
            "New seed",
            false,
            cx,
            |shell, cx| {
                let seed = &mut shell.terrain.settings.generate.seed;
                *seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345) % 16_000_000;
                cx.notify();
            },
        ));
        rows.push(action_button(
            "terrain-generate",
            "Generate",
            true,
            cx,
            |shell, cx| shell.terrain_generate(cx),
        ));
        section("BIOMES", rows).into_any_element()
    }
}
