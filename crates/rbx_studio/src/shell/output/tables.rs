//! A Command Bar run's `print` lines with each printed table drawn as
//! Studio draws it: a chevron and `{...}` that opens into one `[key] = value`
//! line per entry, nested tables opening the same way. Whether a table
//! starts open is Show Tables Expanded by Default; a click flips it (see
//! `OutputLog::table_open`). Show Memory Address puts `table: 0x…` in front.

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex, Icon};
use gpui_kit::*;
use rbx_lua::{PrintPart, PrintValue, TableDump};

use crate::settings::OutputDisplay;
use crate::tokens;

use super::{truncate, OutputEntry, OutputLog, Shell, LABEL_MAX_LEN};

/// How far each level of a table's entries sits in from its brace.
const INDENT: f32 = 14.0;

/// One row per `print` call, its arguments side by side.
pub(super) fn printed(
    entry: &OutputEntry,
    display: OutputDisplay,
    log: &OutputLog,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let mut lines = v_flex().min_w_0().gap_0p5();
    for (line_index, parts) in entry.printed.iter().enumerate() {
        let mut line = h_flex().flex_wrap().items_start().gap_1().min_w_0();
        for (part_index, part) in parts.iter().enumerate() {
            line = line.child(match part {
                PrintPart::Text(text) => div()
                    .min_w_0()
                    .child(truncate(text, LABEL_MAX_LEN))
                    .into_any_element(),
                PrintPart::Table(table) => {
                    let path = vec![line_index as u32, part_index as u32];
                    node(entry.id, path, table, display, log, cx)
                }
            });
        }
        lines = lines.child(line);
    }
    lines
}

/// One table: its header, and when open its entries and closing brace.
/// `path` names it within the entry — the `print` line, the argument, then
/// each entry index on the way down.
fn node(
    id: u64,
    path: Vec<u32>,
    table: &TableDump,
    display: OutputDisplay,
    log: &OutputLog,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let address = if display.table_addresses {
        format!("table: {} ", table.address)
    } else {
        String::new()
    };
    if table.entries.is_empty() && table.more == 0 {
        return div().child(format!("{address}{{}}")).into_any_element();
    }
    let open = log.table_open(id, &path, display.tables_expanded);
    let element_id = SharedString::from(format!("output-table-{id}-{path:?}"));
    let toggle_path = path.clone();
    let header = h_flex()
        .id(element_id)
        .flex_none()
        .items_center()
        .gap_1()
        .cursor_pointer()
        .on_click(cx.listener(move |shell, _, _, cx| {
            cx.stop_propagation();
            shell.output.toggle_table(id, toggle_path.clone());
            cx.notify();
        }))
        .child(
            Icon::new(if open {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            })
            .size(px(10.)),
        )
        .child(format!("{address}{}", if open { "{" } else { "{...}" }));
    if !open {
        return header.into_any_element();
    }

    let mut body = v_flex().min_w_0().child(header);
    let last = table.entries.len().saturating_sub(1);
    for (index, entry) in table.entries.iter().enumerate() {
        let comma = if index < last || table.more > 0 {
            ","
        } else {
            ""
        };
        let value = match &entry.value {
            PrintValue::Text(text) => div()
                .min_w_0()
                .child(format!("{}{comma}", truncate(text, LABEL_MAX_LEN)))
                .into_any_element(),
            PrintValue::Table(inner) => {
                let mut inner_path = path.clone();
                inner_path.push(index as u32);
                node(id, inner_path, inner, display, log, cx)
            }
            PrintValue::Reference(address) => div()
                .text_color(tokens::text_muted())
                .child(format!("table: {address}{comma}"))
                .into_any_element(),
        };
        body = body.child(
            h_flex()
                .pl(px(INDENT))
                .items_start()
                .gap_1()
                .min_w_0()
                .child(div().flex_none().child(format!("[{}] =", entry.key)))
                .child(value),
        );
    }
    if table.more > 0 {
        body = body.child(
            div()
                .pl(px(INDENT))
                .text_color(tokens::text_muted())
                .child(format!("… {} more", table.more)),
        );
    }
    body.child(div().pl(px(4.)).child("}")).into_any_element()
}
