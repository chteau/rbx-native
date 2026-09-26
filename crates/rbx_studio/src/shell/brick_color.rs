//! A part's `BrickColor` row: the colour's swatch and name, opening onto
//! Studio's own picker — the 128 palette colours in a honeycomb, laid out as
//! `honeycomb` establishes — where the arrow keys move in the honeycomb's
//! own geometry, Home and End go to a row's ends (with Ctrl, the picker's),
//! Enter or Space picks, and Escape closes, handing focus back to the row.
//! A pick commits the colour's number through the row like any other edit,
//! which writes the part's `Color` (see `properties::edit::commit_all`).

mod honeycomb;

use gpui_kit::base::actions::Confirm;
use gpui_kit::component::popover::{Popover, PopoverState};
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::BrickColor;

use honeycomb::{Cell, Step};

use super::chrome::Trigger;
use super::rows::select_field;
use super::Shell;
use crate::tokens;

/// A cell's width, point to point across its flat sides, before UI scale.
const CELL: f32 = 20.0;

impl Shell {
    /// `current` is `None` for a multi-selection whose colours differ: the
    /// field shows no colour then, and a pick sets them all.
    ///
    /// A stop in the window's Tab order like any field, and ringed like
    /// every other select (see `rows::select_field`); Enter opens it.
    pub(super) fn brick_color_picker(
        &mut self,
        row: &str,
        current: Option<u32>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + 'static {
        let shown = current.and_then(BrickColor::from_number);
        let focus = self.tab_order.claim(cx);
        let field = select_field(&focus, window, cx)
            .id(SharedString::from(format!("brick-color-{row}")))
            .track_focus(&focus)
            .gap(tokens::label_gap())
            .cursor_pointer()
            .when_some(shown, |this, color| {
                this.child(swatch(color).size(px(12.)).rounded(tokens::radius_tiny()))
                    .child(div().truncate().child(color.name))
            });

        let palette_focus = self
            .edits
            .brick_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        let cursor = self.edits.brick_cursor;
        let handle = cx.entity();
        let row = row.to_owned();
        Popover::new(SharedString::from(format!("brick-color-popover-{row}")))
            .trigger(Trigger::new(field))
            .track_focus(&palette_focus)
            .on_open_change({
                let handle = handle.clone();
                move |open, _, cx| {
                    if *open {
                        handle.update(cx, |shell, cx| {
                            shell.edits.brick_cursor = opening_cell(current);
                            cx.notify();
                        });
                    }
                }
            })
            .content(move |_, _, cx| {
                let picker = Picker {
                    shell: handle.clone(),
                    popover: cx.entity(),
                    row: row.clone(),
                    current,
                };
                picker.render(cursor, &palette_focus)
            })
    }

    fn move_brick_cursor(&mut self, step: Step, cx: &mut Context<Self>) {
        self.edits.brick_cursor =
            honeycomb::step(&honeycomb::cells(), self.edits.brick_cursor, step);
        cx.notify();
    }
}

/// Where the cursor starts: on the row's own colour where the palette holds
/// it, else at the first cell.
fn opening_cell(current: Option<u32>) -> usize {
    let palette = current
        .and_then(BrickColor::from_number)
        .and_then(|color| color.palette);
    honeycomb::cells()
        .iter()
        .position(|cell| Some(cell.palette) == palette)
        .unwrap_or(0)
}

/// What one render of the open picker needs.
struct Picker {
    shell: Entity<Shell>,
    popover: Entity<PopoverState>,
    row: String,
    current: Option<u32>,
}

impl Picker {
    fn pick(&self, palette: u8, window: &mut Window, cx: &mut App) {
        let Some(color) = BrickColor::from_palette(palette) else {
            return;
        };
        let number = color.number.to_string();
        let row = self.row.clone();
        self.shell
            .update(cx, |shell, cx| shell.commit_row(&row, &number, cx));
        self.popover
            .update(cx, |popover, cx| popover.dismiss(window, cx));
    }

    fn render(self, cursor: usize, focus: &FocusHandle) -> impl IntoElement {
        let cells = honeycomb::cells();
        let width = tokens::scaled_width(CELL);
        let geometry = Geometry::new(f32::from(width));
        let focused_name = cells
            .get(cursor)
            .and_then(|cell| BrickColor::from_palette(cell.palette))
            .map_or("", |color| color.name);
        let current = self.current;
        let painted = cells.clone();
        let picker = std::rc::Rc::new(self);

        let key_shell = picker.shell.clone();
        let confirm = picker.clone();
        let confirmed = cells.get(cursor).map(|cell| cell.palette);
        v_flex()
            .p(px(6.))
            .gap(px(6.))
            .child(
                div()
                    .id("brick-color-palette")
                    .track_focus(focus)
                    .relative()
                    .w(px(geometry.width()))
                    .h(px(geometry.height()))
                    .on_key_down(move |event: &KeyDownEvent, _, cx| {
                        let Some(step) = step_for(&event.keystroke) else {
                            return;
                        };
                        cx.stop_propagation();
                        key_shell.update(cx, |shell, cx| shell.move_brick_cursor(step, cx));
                    })
                    // Enter and Space reach the popover as its `Confirm`:
                    // taken here first, so they pick rather than only close.
                    .on_action(move |_: &Confirm, window, cx| {
                        if let Some(palette) = confirmed {
                            confirm.pick(palette, window, cx);
                        }
                    })
                    .child(
                        canvas(
                            |_, _, _| {},
                            move |bounds, _, window, _| {
                                paint(&painted, geometry, bounds.origin, cursor, current, window);
                            },
                        )
                        .absolute()
                        .size_full(),
                    )
                    .children(cells.iter().enumerate().map(|(index, cell)| {
                        let (x, y) = geometry.centre(cell);
                        let hover_shell = picker.shell.clone();
                        let click = picker.clone();
                        let palette = cell.palette;
                        div()
                            .id(("brick-cell", index))
                            .absolute()
                            .left(px(x - geometry.cell / 2.0))
                            .top(px(y - geometry.row / 2.0))
                            .w(px(geometry.cell))
                            .h(px(geometry.row))
                            .cursor_pointer()
                            .on_hover(move |hovered, _, cx| {
                                if *hovered {
                                    hover_shell.update(cx, |shell, cx| {
                                        shell.edits.brick_cursor = index;
                                        cx.notify();
                                    });
                                }
                            })
                            .on_click(move |_, window, cx| click.pick(palette, window, cx))
                    })),
            )
            .child(
                div()
                    .h(tokens::row_height())
                    .text_size(tokens::text_sm())
                    .text_color(tokens::text_label())
                    .child(focused_name),
            )
    }
}

/// The key that moves the cursor, if it is one.
fn step_for(keystroke: &Keystroke) -> Option<Step> {
    let whole = keystroke.modifiers.control || keystroke.modifiers.platform;
    Some(match keystroke.key.as_str() {
        "left" => Step::Left,
        "right" => Step::Right,
        "up" => Step::Up,
        "down" => Step::Down,
        "home" if whole => Step::First,
        "end" if whole => Step::Last,
        "home" => Step::RowStart,
        "end" => Step::RowEnd,
        _ => return None,
    })
}

/// The honeycomb's sizes, from its cell width: pointy-topped hexagons,
/// whose rows sit three quarters of a point-to-point height apart.
#[derive(Debug, Clone, Copy)]
struct Geometry {
    cell: f32,
    radius: f32,
    row: f32,
}

impl Geometry {
    fn new(cell: f32) -> Self {
        let radius = cell / 3f32.sqrt();
        Geometry {
            cell,
            radius,
            row: radius * 1.5,
        }
    }

    /// A cell's centre inside the honeycomb's box. The row of greys sits a
    /// little apart from the hexagon above it, as in Studio.
    fn centre(&self, cell: &Cell) -> (f32, f32) {
        let gap = if cell.row == 13 {
            self.radius * 0.6
        } else {
            0.0
        };
        (
            cell.x * self.cell,
            self.radius + cell.row as f32 * self.row + gap,
        )
    }

    fn width(&self) -> f32 {
        13.0 * self.cell
    }

    fn height(&self) -> f32 {
        self.radius * 2.0 + 13.0 * self.row + self.radius * 0.6
    }

    /// The hexagon around `(x, y)`, shrunk by `inset` to leave a seam.
    fn hexagon(&self, origin: Point<Pixels>, x: f32, y: f32, inset: f32) -> Vec<Point<Pixels>> {
        let radius = self.radius - inset;
        (0..6)
            .map(|corner| {
                let angle =
                    std::f32::consts::FRAC_PI_3 * corner as f32 - std::f32::consts::FRAC_PI_2;
                point(
                    origin.x + px(x + radius * angle.cos()),
                    origin.y + px(y + radius * angle.sin()),
                )
            })
            .collect()
    }
}

fn paint(
    cells: &[Cell],
    geometry: Geometry,
    origin: Point<Pixels>,
    cursor: usize,
    current: Option<u32>,
    window: &mut Window,
) {
    for (index, cell) in cells.iter().enumerate() {
        let Some(color) = BrickColor::from_palette(cell.palette) else {
            continue;
        };
        let (x, y) = geometry.centre(cell);
        let mut fill = PathBuilder::fill();
        fill.add_polygon(&geometry.hexagon(origin, x, y, 0.75), true);
        if let Ok(path) = fill.build() {
            window.paint_path(path, rgba(color));
        }
        // The row's own colour in white, the cursor in the focus colour on
        // top of it — both can be one cell.
        let rings = [
            (Some(color.number) == current, tokens::text_full()),
            (index == cursor, tokens::check_on()),
        ];
        for (_, ring) in rings.iter().filter(|(on, _)| *on) {
            let mut stroke = PathBuilder::stroke(px(2.));
            stroke.add_polygon(&geometry.hexagon(origin, x, y, 1.5), true);
            if let Ok(path) = stroke.build() {
                window.paint_path(path, *ring);
            }
        }
    }
}

fn rgba(color: &BrickColor) -> Rgba {
    let [r, g, b] = color.rgb;
    Rgba {
        r: f32::from(r) / 255.0,
        g: f32::from(g) / 255.0,
        b: f32::from(b) / 255.0,
        a: 1.0,
    }
}

fn swatch(color: &BrickColor) -> Div {
    div().flex_none().bg(rgba(color))
}
