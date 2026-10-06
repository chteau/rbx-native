//! The Terrain Editor's writes: brush strokes and region gestures from the
//! viewport, and the panel's buttons, each landing in `Workspace.Terrain`
//! as a new `SmoothGrid` (and, once a gesture ends, `PhysicsGrid`).
//!
//! A stroke is one undo step, the way a transform drag is: opened by the
//! press, written once at the release. The voxels live in a working copy
//! kept between edits (re-decoded only when something else changed the
//! terrain) with an encoder that re-encodes only the chunks an edit
//! touched. While a gesture runs, each step's changed chunks go straight to
//! the renderer (`Headless::preview_terrain`): no encode, DOM snapshot or
//! decode per mouse move, whatever the size of the map.

mod fields;
mod ops;
mod panel;

use glam::{Mat3, Vec3};
use gpui_kit::*;
use rbx_dom::Ref;
use rbx_terrain::edit::clip::{self, Clip, Placement};
use rbx_terrain::edit::region::StudBox;
use rbx_terrain::smooth_grid::Encoder;
use rbx_terrain::{Before, Cell, ChunkKey, VoxelGrid};
use rbx_viewer::pick::{self, PartSurface, Ray};
use std::collections::HashMap;

use crate::terrain::{
    self, Aim, Effect, Plane, RegionDrag, RegionGrab, Settings, Surfaces, Tab, TerrainTool,
};
use crate::transform::{Action, Tool};
use crate::workspace_view::{Dial, TerrainChunks, TerrainInput, TerrainPhase};

use super::Shell;

pub(super) use fields::TerrainFields;

/// The editor's decoded terrain, kept between edits.
struct Working {
    terrain: Ref,
    grid: VoxelGrid,
    encoder: Encoder,
    physics: rbx_terrain::physics_grid::Encoder,
    /// The `SmoothGrid` bytes `grid` matches: what the editor last wrote, or
    /// last decoded. A DOM holding anything else (an undo, a script) means
    /// decoding again.
    bytes: Vec<u8>,
}

/// What a gesture in progress has changed, beyond the working grid itself:
/// each chunk as it stood before the gesture first reached it, and the
/// revision of each chunk the renderer was last shown.
#[derive(Default)]
struct Touched {
    before: HashMap<ChunkKey, Option<Box<[Cell]>>>,
    shown: HashMap<ChunkKey, Option<u64>>,
}

impl Touched {
    /// Copies the chunks in the voxel box `(min, max)` (exclusive) that have
    /// not been copied yet: called before an edit reaches them. The renderer
    /// already shows each as it is now.
    fn keep(&mut self, grid: &VoxelGrid, (min, max): ([i32; 3], [i32; 3])) {
        let low = ChunkKey::containing(min);
        let high = ChunkKey::containing(max.map(|v| v - 1));
        for x in low.x..=high.x {
            for y in low.y..=high.y {
                for z in low.z..=high.z {
                    let key = ChunkKey { x, y, z };
                    if let std::collections::hash_map::Entry::Vacant(entry) = self.before.entry(key)
                    {
                        entry.insert(grid.chunk(key).map(Box::from));
                        self.shown.insert(key, grid.chunk_revision(key));
                    }
                }
            }
        }
    }

    /// The kept chunks whose voxels changed since the renderer last saw
    /// them, ready to send. (A dirty mark on a chunk never kept is a
    /// neighbour's seam, already covered by sending that neighbour.)
    fn changed(&mut self, grid: &mut VoxelGrid) -> TerrainChunks {
        grid.take_dirty();
        let mut chunks = Vec::new();
        for (key, shown) in &mut self.shown {
            let now = grid.chunk_revision(*key);
            if now != *shown {
                *shown = now;
                chunks.push((*key, grid.chunk(*key).map(Box::from)));
            }
        }
        chunks
    }
}

/// A brush stroke in progress.
struct Stroke {
    working: Working,
    touched: Touched,
    plane: Option<Plane>,
    start_y: f32,
}

/// A Transform drag: the region it lifts, and the chunks the last step
/// changed, which the next step puts back before moving the region again.
struct Lift {
    working: Working,
    touched: Touched,
    source: StudBox,
    last: Vec<ChunkKey>,
}

/// The Terrain Editor's state: which tool, its settings, and the gesture in
/// progress.
pub(crate) struct TerrainEditor {
    pub(super) tab: Tab,
    pub(super) tool: Option<TerrainTool>,
    pub(super) settings: Settings,
    /// Transform's target turn.
    pub(super) rotation: Mat3,
    stroke: Option<Stroke>,
    region_drag: Option<RegionDrag>,
    lift: Option<Lift>,
    /// The region Copy or Cut took, waiting for a Paste.
    pub(super) clipboard: Option<Clip>,
    pub(super) heightmap: Option<std::path::PathBuf>,
    pub(super) colormap: Option<std::path::PathBuf>,
    /// The last ray the cursor cast, so a settings change can redraw the
    /// brush where it already is.
    last_ray: Option<(Ray, Option<rbx_viewer::Pose>, bool)>,
    /// Where the terrain Transform moves now stands, which the next move
    /// lifts from: the region as the tool was entered, or as the last move
    /// left it.
    transform_source: Option<StudBox>,
    /// Where the `Alt`-click material picker stands, while it is open.
    pub(super) picker_at: Option<Point<Pixels>>,
    /// The decoded terrain, between gestures (a gesture holds it meanwhile).
    working: Option<Working>,
}

impl Default for TerrainEditor {
    fn default() -> Self {
        TerrainEditor {
            tab: Tab::Edit,
            tool: None,
            settings: Settings::default(),
            rotation: Mat3::IDENTITY,
            stroke: None,
            region_drag: None,
            lift: None,
            clipboard: None,
            heightmap: None,
            colormap: None,
            last_ray: None,
            transform_source: None,
            picker_at: None,
            working: None,
        }
    }
}

impl TerrainEditor {
    /// Transform's turn as X, Y, Z degrees (applied in that order).
    pub(super) fn rotation_euler(&self) -> (f32, f32, f32) {
        let (x, y, z) = self.rotation.to_euler(glam::EulerRot::XYZ);
        (x.to_degrees(), y.to_degrees(), z.to_degrees())
    }
}

impl Shell {
    /// Home's Terrain group: one tile opening the Terrain Editor (with its
    /// last tool, Draw the first time) or, while it is up, closing it.
    pub(super) fn terrain_tiles(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        use gpui_kit::prelude::FluentBuilder as _;
        let showing = self.is_panel_showing(super::layout::Panel::TerrainEditor);
        vec![super::ribbon::tile(
            &self.ribbon_nav,
            "ribbon-terrain-editor",
            gpui_kit::assets::IconName::Mountain,
            "Terrain Editor",
            cx,
        )
        .when(showing, |this| {
            super::ribbon::selected(this, crate::tokens::tool_scale())
        })
        .tooltip(|window, cx| {
            super::tooltip::text("Terrain Editor — create and sculpt terrain", window, cx)
        })
        .on_click(cx.listener(|shell, _, _, cx| {
            super::ribbon::RibbonCommand::TerrainEditor.run(shell, cx);
        }))
        .into_any_element()]
    }

    /// Whether Ctrl+C/X/V/D and Delete mean the terrain region: the Terrain
    /// Editor's Select tool in use and nothing selected in the Explorer
    /// (`terrain-editor.md`'s own condition).
    pub(super) fn terrain_select_keys(&self) -> bool {
        self.transform.tool == Tool::Terrain
            && self.terrain.tool == Some(TerrainTool::Select)
            && self.selected_all().is_empty()
    }

    pub(super) fn toggle_terrain_editor(&mut self, open: bool, cx: &mut Context<Self>) {
        self.set_panel_open(super::layout::Panel::TerrainEditor, open, cx);
        if open {
            let tool = self.terrain.tool.unwrap_or(TerrainTool::Draw);
            self.use_terrain_tool(tool, cx);
        } else if self.transform.tool == Tool::Terrain {
            self.transform_action(Action::Use(Tool::Select), cx);
        }
    }

    /// `RBX_STUDIO_TERRAIN=<tool>[,apply]` opens the Terrain Editor on that
    /// tool at startup, and with `apply` presses its main button (Generate,
    /// Fill, Sea Level's Create, Clear, Import) — a screenshot aid, as
    /// `RBX_STUDIO_TOOL` is for the transform tools.
    pub(super) fn apply_debug_terrain(&mut self, cx: &mut Context<Self>) {
        let Ok(spec) = std::env::var("RBX_STUDIO_TERRAIN") else {
            return;
        };
        let mut words = spec.split(',').map(|w| w.trim().to_ascii_lowercase());
        let Some(name) = words.next() else { return };
        let all = TerrainTool::CREATE.into_iter().chain(TerrainTool::EDIT);
        let Some(tool) = all
            .into_iter()
            .find(|t| t.label().replace(' ', "").eq_ignore_ascii_case(&name))
        else {
            eprintln!("rbxstudio: RBX_STUDIO_TERRAIN: no tool called {name:?}");
            return;
        };
        self.set_panel_open(super::layout::Panel::TerrainEditor, true, cx);
        self.use_terrain_tool(tool, cx);
        if words.any(|w| w == "apply") {
            match tool {
                TerrainTool::Generate => self.terrain_generate(cx),
                TerrainTool::Fill => self.terrain_fill(cx),
                TerrainTool::SeaLevel => self.terrain_sea_level(true, cx),
                TerrainTool::Clear => self.terrain_clear(cx),
                TerrainTool::Import => self.terrain_import(cx),
                _ => {}
            }
        }
    }

    /// Picks a Terrain Editor tool and puts the viewport in its hands.
    pub(super) fn use_terrain_tool(&mut self, tool: TerrainTool, cx: &mut Context<Self>) {
        self.terrain.tool = Some(tool);
        self.terrain.tab = tool.tab();
        self.terrain.stroke = None;
        self.terrain.region_drag = None;
        if tool == TerrainTool::Transform {
            self.terrain.rotation = Mat3::IDENTITY;
            self.terrain.transform_source = Some(self.terrain.settings.active_region());
        }
        self.transform_action(Action::Use(Tool::Terrain), cx);
        self.redraw_terrain_overlay(cx);
    }

    /// Brings the working copy in line with the DOM, decoding only when the
    /// terrain's bytes are not the ones it already holds. `Err` says why
    /// there is nothing to edit.
    fn sync_working(&mut self) -> Result<(), String> {
        let terrain =
            terrain::find_terrain(&self.dom).ok_or("this place has no Workspace.Terrain")?;
        let bytes = terrain::grid_bytes(&self.dom, terrain);
        if let Some(working) = &self.terrain.working {
            if working.terrain == terrain && working.bytes == bytes {
                return Ok(());
            }
        }
        self.terrain.working = None;
        let grid = if bytes.is_empty() {
            VoxelGrid::new()
        } else {
            rbx_terrain::smooth_grid::decode(bytes).map_err(|why| {
                format!(
                    "the terrain's voxels could not be read ({why}); editing would \
                     discard them, so nothing was changed"
                )
            })?
        };
        self.terrain.working = Some(Working {
            terrain,
            grid,
            encoder: Encoder::default(),
            physics: Default::default(),
            bytes: bytes.to_vec(),
        });
        Ok(())
    }

    /// The working copy, taken for an edit (put back by `check_in`), or a
    /// warning in Output saying why there is nothing to edit.
    fn check_out(&mut self) -> Option<Working> {
        if let Err(why) = self.sync_working() {
            self.output.push_warning(&format!("Terrain Editor: {why}"));
            return None;
        }
        self.terrain.working.take()
    }

    /// The terrain to read from, current with the DOM.
    pub(super) fn terrain_voxels(&mut self) -> Option<&VoxelGrid> {
        if let Err(why) = self.sync_working() {
            self.output.push_warning(&format!("Terrain Editor: {why}"));
            return None;
        }
        self.terrain.working.as_ref().map(|w| &w.grid)
    }

    /// Writes the working copy back (with `PhysicsGrid`, which only a
    /// finished edit needs) and keeps it for the next edit.
    fn check_in(&mut self, mut working: Working, physics: bool, cx: &mut Context<Self>) {
        let bytes = working.encoder.encode(&working.grid);
        working.grid.take_dirty();
        let physics = physics.then(|| working.physics.encode(&working.grid));
        if let Err(err) =
            terrain::write_encoded(&mut self.dom, working.terrain, bytes.clone(), physics)
        {
            self.output.push_warning(&format!("Terrain Editor: {err}"));
        }
        working.bytes = bytes;
        self.terrain.working = Some(working);
        let changes = self.dom.take_changes();
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
    }

    /// One whole edit as one undo step: read, change, write.
    pub(super) fn edit_terrain(
        &mut self,
        edit: impl FnOnce(&mut VoxelGrid),
        cx: &mut Context<Self>,
    ) {
        let Some(mut working) = self.check_out() else {
            cx.notify();
            return;
        };
        self.push_history();
        edit(&mut working.grid);
        self.check_in(working, true, cx);
        cx.notify();
    }

    fn preview(&self, chunks: TerrainChunks, cx: &mut Context<Self>) {
        self.viewport
            .update(cx, |viewport, _| viewport.preview_terrain(chunks));
    }

    /// Everything the viewport sends while the Terrain Editor has it.
    pub(super) fn terrain_step(&mut self, input: TerrainInput, cx: &mut Context<Self>) {
        let Some(tool) = self.terrain.tool else {
            return;
        };
        if let Some(ray) = input.ray {
            self.terrain.last_ray = Some((ray, input.pose, input.orthographic));
        }
        match input.phase {
            TerrainPhase::Adjust { dial, notches } => self.adjust_dial(dial, notches, cx),
            TerrainPhase::Picker { x, y } => {
                if tool.is_brush() {
                    self.terrain.picker_at = Some(point(px(x), px(y)));
                    cx.notify();
                }
            }
            TerrainPhase::Delete => {
                if self.terrain_select_keys() {
                    self.terrain_delete_region(cx);
                }
            }
            _ if tool.is_brush() => self.brush_step(tool, input, cx),
            _ if tool.uses_region() => self.region_step(tool, input, cx),
            _ => {}
        }
    }

    fn adjust_dial(&mut self, dial: Dial, notches: f32, cx: &mut Context<Self>) {
        let settings = &mut self.terrain.settings;
        match dial {
            Dial::Size => settings.set_size(settings.size + notches),
            Dial::Height => settings.set_height(settings.height + notches),
            Dial::Strength => settings.set_strength(settings.strength + notches * 0.05),
        }
        self.redraw_terrain_overlay(cx);
        cx.notify();
    }

    fn aim_brush(&self, ray: Ray, plane: Option<Plane>, cx: &App) -> Option<Aim> {
        let meshes = self.viewport.read(cx).meshes().clone();
        let part = |ray: Ray| {
            pick::parts_along(&self.dom, &self.database, &meshes, ray)
                .into_iter()
                .find_map(|part| {
                    PartSurface::read(&self.dom, &self.database, &meshes, part)?.raycast(ray)
                })
        };
        let empty = VoxelGrid::new();
        let before;
        let grid: &dyn rbx_terrain::Voxels = match &self.terrain.stroke {
            Some(stroke) => {
                before = Before {
                    grid: &stroke.working.grid,
                    chunks: &stroke.touched.before,
                };
                &before
            }
            None => self.terrain.working.as_ref().map_or(&empty, |w| &w.grid),
        };
        let surfaces = Surfaces {
            grid,
            part: Some(&part),
        };
        terrain::aim(&self.terrain.settings, &surfaces, ray, plane)
    }

    fn brush_step(&mut self, tool: TerrainTool, input: TerrainInput, cx: &mut Context<Self>) {
        match input.phase {
            TerrainPhase::Hover => {
                let _ = self.sync_working();
                let aim = input.ray.and_then(|ray| self.aim_brush(ray, None, cx));
                self.show_brush(aim, input.ctrl, input.shift, cx);
            }
            TerrainPhase::Press => {
                let Some(ray) = input.ray else { return };
                let Some(working) = self.check_out() else {
                    return;
                };
                self.terrain.stroke = Some(Stroke {
                    working,
                    touched: Touched::default(),
                    plane: None,
                    start_y: 0.0,
                });
                let Some(aim) = self.aim_brush(ray, None, cx) else {
                    self.finish_stroke(false, cx);
                    return;
                };
                let forward = input.pose.map_or(Vec3::NEG_Z, |pose| pose.basis().2);
                let plane = terrain::stroke_plane(&self.terrain.settings, aim.hit, forward);
                if let Some(stroke) = &mut self.terrain.stroke {
                    stroke.plane = plane;
                    stroke.start_y = aim.hit.y;
                }
                self.push_history();
                self.apply_stroke(tool, aim, input, cx);
            }
            TerrainPhase::Drag => {
                let Some(ray) = input.ray else { return };
                let plane = self.terrain.stroke.as_ref().and_then(|s| s.plane);
                if self.terrain.stroke.is_none() {
                    return;
                }
                if let Some(aim) = self.aim_brush(ray, plane, cx) {
                    self.apply_stroke(tool, aim, input, cx);
                }
            }
            TerrainPhase::Release => self.finish_stroke(true, cx),
            TerrainPhase::Adjust { .. } | TerrainPhase::Delete | TerrainPhase::Picker { .. } => {}
        }
    }

    /// Ends the stroke: written (`write`), or — a press that found nothing
    /// to aim at — just put back.
    fn finish_stroke(&mut self, write: bool, cx: &mut Context<Self>) {
        let Some(stroke) = self.terrain.stroke.take() else {
            return;
        };
        if write {
            self.check_in(stroke.working, true, cx);
        } else {
            self.terrain.working = Some(stroke.working);
        }
    }

    fn apply_stroke(
        &mut self,
        tool: TerrainTool,
        aim: Aim,
        input: TerrainInput,
        cx: &mut Context<Self>,
    ) {
        let Some(mut stroke) = self.terrain.stroke.take() else {
            return;
        };
        let settings = &self.terrain.settings;
        if let Some(effect) = Effect::of(tool, settings, input.ctrl, input.shift, stroke.start_y) {
            let brush = terrain::brush(settings, aim.center);
            // Smooth reads a voxel past the brush; the copy takes one more.
            let (min, max) = brush.voxel_bounds();
            stroke.touched.keep(
                &stroke.working.grid,
                (min.map(|v| v - 1), max.map(|v| v + 1)),
            );
            terrain::apply_brush(&mut stroke.working.grid, settings, effect, aim.center);
        }
        let chunks = stroke.touched.changed(&mut stroke.working.grid);
        self.terrain.stroke = Some(stroke);
        self.preview(chunks, cx);
        self.show_brush(Some(aim), input.ctrl, input.shift, cx);
    }

    fn show_brush(&mut self, aim: Option<Aim>, ctrl: bool, shift: bool, cx: &mut Context<Self>) {
        let subtract = match self.terrain.tool {
            Some(TerrainTool::Draw | TerrainTool::Sculpt) => {
                (self.terrain.settings.mode == terrain::BrushMode::Subtract) != ctrl && !shift
            }
            _ => false,
        };
        let segments = aim
            .map(|aim| terrain::brush_outline(&self.terrain.settings, aim.center, subtract))
            .unwrap_or_default();
        self.viewport
            .update(cx, |viewport, _| viewport.show_terrain(segments));
    }

    fn region_step(&mut self, tool: TerrainTool, input: TerrainInput, cx: &mut Context<Self>) {
        let pose = input.pose;
        match input.phase {
            TerrainPhase::Hover
            | TerrainPhase::Adjust { .. }
            | TerrainPhase::Delete
            | TerrainPhase::Picker { .. } => {}
            TerrainPhase::Press => {
                let (Some(ray), Some(pose)) = (input.ray, pose) else {
                    return;
                };
                let _ = self.sync_working();
                let surface = self
                    .aim_brush(ray, None, cx)
                    .map_or(Vec3::ZERO, |aim| aim.hit);
                let transform = tool == TerrainTool::Transform;
                let drag = RegionDrag::press(
                    &self.terrain.settings.region,
                    self.terrain.rotation,
                    (pose, input.orthographic),
                    ray,
                    transform,
                    surface,
                );
                // Transform's own gesture moves terrain; drawing a new box
                // with it only re-selects.
                if transform && !matches!(drag.grab, RegionGrab::Draw { .. }) {
                    if let Some(working) = self.check_out() {
                        self.push_history();
                        let source = self
                            .terrain
                            .transform_source
                            .unwrap_or_else(|| self.terrain.settings.active_region());
                        self.terrain.lift = Some(Lift {
                            working,
                            touched: Touched::default(),
                            source,
                            last: Vec::new(),
                        });
                    }
                }
                self.terrain.region_drag = Some(drag);
            }
            TerrainPhase::Drag => {
                let (Some(ray), Some(mut drag)) = (input.ray, self.terrain.region_drag) else {
                    return;
                };
                let snap = self.terrain.settings.region_snap;
                let (region, rotation) = drag.step(ray, input.shift, input.ctrl, snap);
                self.terrain.region_drag = Some(drag);
                self.terrain.settings.region = region;
                self.terrain.rotation = rotation;
                if self.terrain.settings.live_edit {
                    self.apply_lift(cx);
                }
            }
            TerrainPhase::Release => {
                self.terrain.region_drag = None;
                if self.terrain.lift.is_some() {
                    self.apply_lift(cx);
                    if let Some(lift) = self.terrain.lift.take() {
                        self.check_in(lift.working, true, cx);
                    }
                    if self.terrain.settings.live_edit {
                        self.terrain.transform_source = Some(self.terrain.settings.region);
                    }
                }
            }
        }
        self.redraw_terrain_overlay(cx);
        cx.notify();
    }

    fn placement(&self) -> Placement {
        Placement {
            center: self.terrain.settings.region.center(),
            size: self.terrain.settings.region.size(),
            axes: [
                self.terrain.rotation.x_axis.to_array(),
                self.terrain.rotation.y_axis.to_array(),
                self.terrain.rotation.z_axis.to_array(),
            ],
        }
    }

    /// Transform's Apply (with Live Edit off, or a typed position, size or
    /// turn): the source region's terrain moved to where the region now
    /// stands, as one undo step.
    pub(super) fn terrain_apply_transform(&mut self, cx: &mut Context<Self>) {
        let Some(source) = self.terrain.transform_source else {
            return;
        };
        let placement = self.placement();
        let merge = self.terrain.settings.merge_empty;
        self.edit_terrain(|grid| clip::transform(grid, &source, &placement, merge), cx);
        self.terrain.transform_source = Some(self.terrain.settings.region);
        self.terrain.rotation = Mat3::IDENTITY;
        self.redraw_terrain_overlay(cx);
    }

    /// A number typed into the panel.
    pub(super) fn set_terrain_number(
        &mut self,
        number: fields::Number,
        value: f32,
        cx: &mut Context<Self>,
    ) {
        use fields::Number;
        if !value.is_finite() {
            return;
        }
        let settings = &mut self.terrain.settings;
        let mut center = settings.region.center();
        let mut size = settings.region.size();
        match number {
            Number::Position(axis) => center[axis] = value,
            Number::Size(axis) => size[axis] = value.max(rbx_terrain::VOXEL_STUDS),
            Number::Rotation(axis) => {
                let (x, y, z) = self.terrain.rotation_euler();
                let mut angles = [x, y, z];
                angles[axis] = value;
                self.terrain.rotation = Mat3::from_euler(
                    glam::EulerRot::XYZ,
                    angles[0].to_radians(),
                    angles[1].to_radians(),
                    angles[2].to_radians(),
                );
            }
            Number::PlaneOrigin(axis) => settings.plane_origin[axis] = value,
            Number::PlaneTilt(axis) => {
                let mut tilt = crate::terrain::plane_tilt(settings.plane_normal);
                tilt[axis] = value;
                settings.plane_normal = crate::terrain::plane_normal(tilt);
            }
            Number::FlattenY => settings.flatten_y = value,
            Number::Seed => settings.generate.seed = value.max(0.0) as u32,
        }
        if matches!(number, Number::Position(_) | Number::Size(_)) {
            self.terrain.settings.region = StudBox::from_center_size(center, size);
        }
        let moves = matches!(
            number,
            Number::Position(_) | Number::Size(_) | Number::Rotation(_)
        );
        if moves
            && self.terrain.tool == Some(TerrainTool::Transform)
            && self.terrain.settings.live_edit
        {
            self.terrain_apply_transform(cx);
        }
        self.redraw_terrain_overlay(cx);
        cx.notify();
    }

    /// Re-applies the Transform drag: the chunks the last step changed put
    /// back as they were, then the region moved to where it now stands —
    /// work in proportion to the region, not the map.
    fn apply_lift(&mut self, cx: &mut Context<Self>) {
        let placement = self.placement();
        let merge = self.terrain.settings.merge_empty;
        let Some(lift) = &mut self.terrain.lift else {
            return;
        };
        for key in std::mem::take(&mut lift.last) {
            if let Some(original) = lift.touched.before.get(&key) {
                lift.working.grid.replace_chunk(key, original.clone());
            }
        }
        lift.touched
            .keep(&lift.working.grid, lift.source.snapped().voxels());
        lift.touched
            .keep(&lift.working.grid, placement.bounds().voxels());
        clip::transform(&mut lift.working.grid, &lift.source, &placement, merge);
        let chunks = lift.touched.changed(&mut lift.working.grid);
        lift.last = lift.touched.before.keys().copied().collect();
        self.preview(chunks, cx);
    }

    /// Redraws whatever the active tool shows: the region and its handles,
    /// or the brush where the cursor last was.
    pub(super) fn redraw_terrain_overlay(&mut self, cx: &mut Context<Self>) {
        if self.transform.tool != Tool::Terrain {
            self.viewport
                .update(cx, |viewport, _| viewport.show_terrain(Vec::new()));
            return;
        }
        let Some(tool) = self.terrain.tool else {
            return;
        };
        if tool.uses_region() {
            let region = self.terrain.settings.region;
            let rotation = self.terrain.rotation;
            let model = terrain::region_model(&region, rotation);
            let segments = match self
                .terrain
                .last_ray
                .and_then(|(_, pose, ortho)| Some((pose?, ortho)))
            {
                Some((pose, ortho)) => {
                    let (faces, handles) = terrain::region_handles(&region, rotation, pose, ortho);
                    let transform = tool == TerrainTool::Transform;
                    terrain::region_outline(model, Some(&faces), transform.then_some(&handles))
                }
                None => terrain::region_outline(model, None, None),
            };
            self.viewport
                .update(cx, |viewport, _| viewport.show_terrain(segments));
        } else if tool.is_brush() {
            let aim = self
                .terrain
                .last_ray
                .and_then(|(ray, ..)| self.aim_brush(ray, None, cx));
            self.show_brush(aim, false, false, cx);
        } else {
            self.viewport
                .update(cx, |viewport, _| viewport.show_terrain(Vec::new()));
        }
    }
}
