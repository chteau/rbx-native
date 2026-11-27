//! One ribbon control per [`Item`]. The tiles that belong to a feature
//! (Terrain, Pivot, Sun, the solid modeling tools) are built in that
//! feature's module; this is the rest, and the dispatch.

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::commands::{RibbonCommand, EFFECT_INSERTS, PART_INSERTS};
use super::layout::Item;
use super::{
    cloud_tile, cluster_icon, insert_item, model_tiles, refused_icon, script_items, selected, tile,
    tool_accent, tool_icon, unavailable_tile,
};
use crate::shell::chrome::Trigger;
use crate::shell::menu::{self, MenuId};
use crate::shell::{tooltip, Shell};
use crate::tokens;
use crate::transform::Tool;

impl Shell {
    pub(super) fn ribbon_item(&mut self, item: Item, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let one = match item {
            Item::Tools => self.tools_cluster(cx),
            Item::Snap => self.snap_stack(cx).into_any_element(),
            Item::Part => self.part_menu(cx),
            Item::Terrain => return self.terrain_tiles(cx),
            Item::Character => return self.rig_tiles(cx),
            Item::Gui => self.insert_gui(cx).into_any_element(),
            Item::Script => self.script_menu(cx),
            Item::Import => return self.import_tiles(cx),
            Item::Material => self.material_tile(cx),
            Item::Color => self.color_tile(cx),
            Item::Group => self.group_tile(cx),
            Item::Ungroup => self.ungroup_tile(cx),
            Item::Lock => self.flag_tile(model_tiles::Flag::Locked, cx),
            Item::Anchor => self.flag_tile(model_tiles::Flag::Anchored, cx),
            Item::Dock(panel) => self.dock_tile(panel, cx),
            Item::DataStores => cloud_tile(
                &self.ribbon_nav,
                "ribbon-data-stores",
                IconName::Database,
                "Data Stores",
                "Data Stores \u{2014} browse and edit the experience's data stores",
                |shell, cx| RibbonCommand::DataStores.run(shell, cx),
                cx,
            ),
            Item::UpdatePlanning => self.labelled(
                "ribbon-update-planning",
                IconName::CalendarClock,
                "Update Planning",
                "Update Planning \u{2014} countdown icons swapped in on schedule",
                |shell, _, cx| RibbonCommand::UpdatePlanning.run(shell, cx),
                cx,
            ),
            Item::AvatarSettings => self.avatar_settings_tile(cx),
            Item::Setup => self.avatar_setup_tile(cx),
            Item::Accessory => self.labelled(
                "ribbon-accessory",
                IconName::Package,
                "Accessory",
                "Accessory \u{2014} fit a mesh to a body as an accessory or layered clothing, test it, and generate it",
                |shell, _, cx| shell.open_ugc_fitting(None, None, cx),
                cx,
            ),
            Item::UgcValidation => self.labelled(
                "ribbon-check-item",
                IconName::ShieldCheck,
                "UGC Validation",
                "UGC Validation \u{2014} check an avatar item against Roblox\u{2019}s Marketplace limits",
                |shell, _, cx| shell.open_ugc_validator(None, None, cx),
                cx,
            ),
            Item::UploadItem => cloud_tile(
                &self.ribbon_nav,
                "ribbon-upload-item",
                IconName::CloudUpload,
                "Upload Item",
                "Upload Item \u{2014} upload an avatar item and finish it on the Creator Hub",
                |shell, cx| shell.open_ugc_upload(None, None, cx),
                cx,
            ),
            Item::FacePreview => self.labelled(
                "ribbon-face-preview",
                IconName::ScanFace,
                "Face Preview",
                "Face Preview \u{2014} pose a dynamic head\u{2019}s FACS controls and see its landmarks",
                |shell, _, cx| shell.open_face_preview(cx),
                cx,
            ),
            Item::ClipEditor => self.animation_tile(cx),
            Item::StyleEditor => self.style_editor_tile(cx),
            Item::PseudoLoc => self.pseudo_loc_tile(cx),
            Item::Format => self.format_tile(cx),
            Item::Find => self.find_menu(cx),
            Item::CommandBar => self.command_bar_tile(cx),
            Item::Pivot => return self.pivot_tiles(cx),
            Item::Effect => menu::dropdown_in_group(
                self,
                MenuId::InsertEffect,
                Trigger::new(tile(
                    &self.ribbon_nav,
                    "ribbon-insert-effect",
                    IconName::WandSparkles,
                    "Effect",
                    cx,
                )),
                EFFECT_INSERTS.iter().map(insert_item).collect(),
                cx,
            )
            .into_any_element(),
            Item::Align => self
                .align_popover(
                    Trigger::new(tile(
                        &self.ribbon_nav,
                        "tool-align",
                        IconName::AlignStartVertical,
                        "Align",
                        cx,
                    ))
                    .tooltip("Align \u{2014} line the selection up")
                    .accent(tokens::tool_align()),
                    cx,
                )
                .into_any_element(),
            Item::SolidModeling => return self.solid_modeling_tiles(cx),
            Item::Sun => return self.sun_tiles(cx),
            Item::Test => return self.test_tiles(cx),
            Item::ViewportSettings => self.labelled(
                "ribbon-viewport-settings",
                IconName::Settings,
                "Settings",
                "Settings \u{2014} the viewport's settings",
                |shell, _, cx| shell.open_settings_on("viewport", cx),
                cx,
            ),
            Item::ShowUi => self.show_ui_tile(cx),
            Item::Fullbright => self.fullbright_tile(cx),
            Item::PluginToolbars => {
                return self.plugin_toolbar_groups(cx).into_iter().flatten().collect()
            }
            Item::PluginTools => return self.plugin_tools(cx),
        };
        vec![one]
    }

    /// A plain live tile with its hover text.
    fn labelled(
        &self,
        id: &'static str,
        icon: IconName,
        label: &'static str,
        tip: &'static str,
        on_click: impl Fn(&mut Shell, &mut Window, &mut Context<Shell>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        tile(&self.ribbon_nav, id, icon, label, cx)
            .tooltip(move |window, cx| tooltip::text(tip, window, cx))
            .on_click(cx.listener(move |shell, _, window, cx| on_click(shell, window, cx)))
            .into_any_element()
    }

    /// The five transform tools and the Local toggle, in one bordered
    /// cluster, icon only: each keeps its own tooltip, so nothing loses its
    /// accessible name by dropping a caption nobody needs to read twice.
    fn tools_cluster(&self, cx: &mut Context<Self>) -> AnyElement {
        let active = self.transform.effective();
        let cluster: Vec<AnyElement> = Tool::TRANSFORM
            .map(|tool| {
                let refusal = self.transform.refusal(tool);
                let id = ("tool", tool as usize);
                match refusal {
                    Some(_) => refused_icon(&self.ribbon_nav, id, tool_icon(tool), cx),
                    None => cluster_icon(&self.ribbon_nav, id, tool_icon(tool), cx),
                }
                .when(active == tool, |this| selected(this, tool_accent(tool)))
                .tooltip(move |window, cx| {
                    let text = match (refusal, tool.shortcut()) {
                        (Some(reason), _) => format!("{} — {reason}", tool.label()),
                        (None, Some(key)) => format!("{} ({key})", tool.label()),
                        (None, None) => tool.label().to_owned(),
                    };
                    tooltip::text(text, window, cx)
                })
                .on_click(cx.listener(move |shell, _, _, cx| {
                    shell.transform_action(crate::transform::Action::Use(tool), cx);
                }))
                .into_any_element()
            })
            .into_iter()
            .chain([self.local_tile(cx).into_any_element()])
            .collect();

        v_flex()
            .flex_none()
            .h_full()
            .justify_center()
            .child(
                h_flex()
                    .flex_none()
                    .items_center()
                    .gap(px(4.))
                    .p(px(5.))
                    .bg(tokens::field_select())
                    .rounded(tokens::radius_container())
                    .border_1()
                    .border_color(tokens::border())
                    .children(cluster),
            )
            .into_any_element()
    }

    /// Part's menu. Every item routes through `RibbonCommand::Insert`, the
    /// same path the command palette lists it under.
    fn part_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        menu::dropdown_in_group(
            self,
            MenuId::InsertPart,
            Trigger::new(tile(
                &self.ribbon_nav,
                "ribbon-insert-part",
                IconName::Box,
                "Part",
                cx,
            )),
            PART_INSERTS.iter().map(insert_item).collect(),
            cx,
        )
        .into_any_element()
    }

    fn script_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        menu::dropdown_in_group(
            self,
            MenuId::InsertScript,
            Trigger::new(tile(
                &self.ribbon_nav,
                "ribbon-insert-script",
                IconName::FileCode,
                "Script",
                cx,
            )),
            script_items(&self.script_templates),
            cx,
        )
        .into_any_element()
    }

    /// Group and Ungroup grey exactly while their handlers would return
    /// early: `group::has_groupable` and `has_ungroupable` are the guards
    /// `group_selected` and `ungroup_selected` check, so a live tile always
    /// has work to do.
    fn group_tile(&self, cx: &mut Context<Self>) -> AnyElement {
        if super::super::group::has_groupable(&self.dom, &self.database, self.selected_all()) {
            tile(
                &self.ribbon_nav,
                "ribbon-group",
                IconName::Group,
                "Group",
                cx,
            )
            .on_click(cx.listener(|shell, _, _, cx| shell.group_selected(cx)))
        } else {
            unavailable_tile(
                "ribbon-group",
                IconName::Group,
                "Group",
                "nothing groupable is selected",
            )
        }
        .into_any_element()
    }

    fn ungroup_tile(&self, cx: &mut Context<Self>) -> AnyElement {
        if super::super::group::has_ungroupable(&self.dom, &self.database, self.selected_all()) {
            tile(
                &self.ribbon_nav,
                "ribbon-ungroup",
                IconName::Ungroup,
                "Ungroup",
                cx,
            )
            .on_click(cx.listener(|shell, _, _, cx| shell.ungroup_selected(cx)))
        } else {
            unavailable_tile(
                "ribbon-ungroup",
                IconName::Ungroup,
                "Ungroup",
                "no Model is selected",
            )
        }
        .into_any_element()
    }

    /// Runs `f` with this editor's window, once the current update is done:
    /// for the commands a menu row or a menu-bar item starts that need the
    /// window (focus, a field to type in) but only get the `Shell`.
    pub(in crate::shell) fn with_window(
        &self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Shell, &mut Window, &mut Context<Shell>) + 'static,
    ) {
        let handle = self.window_handle;
        let shell = cx.entity().downgrade();
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| {
                let _ = shell.update(cx, |shell, cx| f(shell, window, cx));
            });
        });
    }
}
