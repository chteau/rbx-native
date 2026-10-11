//! Paints the dope sheet's rows: the clip's span, the grid, the selected
//! channel, each joint's keys and the playhead.

use gpui_kit::*;

use super::{Lines, KEY, ROW};
use crate::timeline::{paint, Timeline};
use crate::tokens;

pub(super) fn paint_rows(
    window: &mut Window,
    b: Bounds<Pixels>,
    tl: &Timeline,
    lines: &Lines,
    time: f32,
) {
    let (ox, oy) = (f32::from(b.origin.x), f32::from(b.origin.y));
    let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
    // Outside the clip's span, shaded: Blender's frame-range dimming.
    let (start, end) = (tl.x_of(0.0), tl.x_of(lines.length.max(0.0)));
    paint::rect(window, ox, oy, start.clamp(0.0, w), h, tokens::wash_faint());
    paint::rect(
        window,
        ox + end.clamp(0.0, w),
        oy,
        (w - end).max(0.0),
        h,
        tokens::wash_faint(),
    );
    paint::rect(window, ox, oy, w, ROW, tokens::wash_faint());
    if let Some(r) = lines.selected_row {
        paint::rect(
            window,
            ox,
            oy + (r + 1) as f32 * ROW,
            w,
            ROW,
            tokens::accent_soft(),
        );
    }
    for tick in tl.ticks(w).into_iter().filter(|t| t.major) {
        paint::rect(window, ox + tick.x, oy, 1.0, h, tokens::border());
    }
    for r in 0..=lines.rows.len() {
        paint::rect(
            window,
            ox,
            oy + (r + 1) as f32 * ROW - 1.0,
            w,
            1.0,
            tokens::border(),
        );
    }
    let key_fill = |selected: bool| {
        if selected {
            tokens::check_on()
        } else {
            tokens::text_label()
        }
    };
    for (t, selected) in &lines.summary {
        let x = ox + tl.x_of(*t);
        paint::diamond(
            window,
            x,
            oy + ROW * 0.5,
            KEY + 1.0,
            key_fill(*selected),
            tokens::black(),
        );
    }
    for (r, (keys, selected)) in lines.rows.iter().enumerate() {
        let y = oy + (r + 1) as f32 * ROW + ROW * 0.5;
        // A bar between two keys holding the same pose would need values;
        // the run between first and last key is drawn instead, as the span
        // the joint is animated over.
        if let (Some(first), Some(last)) = (keys.first(), keys.last()) {
            if last > first {
                paint::rect(
                    window,
                    ox + tl.x_of(*first),
                    y - 1.0,
                    tl.x_of(*last) - tl.x_of(*first),
                    2.0,
                    tokens::border2(),
                );
            }
        }
        for (t, s) in keys.iter().zip(selected) {
            paint::diamond(
                window,
                ox + tl.x_of(*t),
                y,
                KEY,
                key_fill(*s),
                tokens::black(),
            );
        }
    }
    paint::playhead(window, b.origin, tl, time, h);
}
