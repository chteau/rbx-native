//! The horizontal scrollbar under the dope sheet and the Curve Editor: what
//! range the timeline scrolls over, where its thumb sits, and how a thumb
//! drag or a click on the track moves the view. The view itself is still
//! just [`Timeline::start`] and [`Timeline::scale`]; this only maps them to
//! and from the bar.
//!
//! The range runs past the clip's End, as in Roblox's Animation Editor,
//! whose timeline scrolls on past the last key so a key can be placed
//! there: the End plus half the clip again, and at least
//! [`PAST_END_MIN`] seconds past it.

use super::{Timeline, LEAD};

/// The least room past the clip's End the timeline scrolls over, in seconds.
pub(crate) const PAST_END_MIN: f32 = 2.0;

/// The shortest the thumb is drawn, so a long range still leaves something
/// to grab.
pub(crate) const MIN_THUMB: f32 = 24.0;

/// Of a page, how much stays on screen after a click on the track.
const PAGE_OVERLAP: f32 = 0.1;

/// A stretch of the timeline, in seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Span {
    pub(crate) lo: f32,
    pub(crate) hi: f32,
}

impl Span {
    pub(crate) fn len(&self) -> f32 {
        (self.hi - self.lo).max(0.0)
    }
}

/// The thumb on a track, in pixels from the track's left edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Thumb {
    pub(crate) x: f32,
    pub(crate) w: f32,
}

impl Thumb {
    pub(crate) fn contains(&self, x: f32) -> bool {
        (self.x..=self.x + self.w).contains(&x)
    }
}

impl Timeline {
    /// Seconds shown across `width` pixels.
    pub(crate) fn visible(&self, width: f32) -> f32 {
        width.max(0.0) / self.scale
    }

    /// Where the view may go for a clip `length` seconds long: from just
    /// before frame 0 (the room `fit` leaves) to the End plus half the clip
    /// again, and never less than [`PAST_END_MIN`] past it.
    pub(crate) fn content_span(&self, length: f32) -> Span {
        let length = length.max(0.0);
        Span {
            lo: -LEAD / self.scale,
            hi: length + (length * 0.5).max(PAST_END_MIN),
        }
    }

    /// The range the scrollbar stands for: [`Self::content_span`], widened
    /// to hold whatever is on screen, so a view zoomed out past the range
    /// shows a full-length thumb rather than one off the end of the track.
    pub(crate) fn scroll_span(&self, length: f32, width: f32) -> Span {
        let content = self.content_span(length);
        Span {
            lo: content.lo.min(self.start),
            hi: content.hi.max(self.start + self.visible(width)),
        }
    }

    /// Keeps the view inside `span`; a view wider than the span starts at
    /// its left edge.
    pub(crate) fn clamp_to(&mut self, span: Span, width: f32) {
        // Zooming out stops once the whole range fits the view.
        if width > 0.0 && span.len() > 0.0 {
            let floor = (width / span.len()).min(super::SCALE_RANGE.1);
            if self.scale < floor {
                self.scale = floor;
            }
        }
        let last = span.hi - self.visible(width);
        self.start = if last <= span.lo {
            span.lo
        } else {
            self.start.clamp(span.lo, last)
        };
    }

    /// The thumb for this view on a `track` pixels wide standing for `span`,
    /// the view being `width` pixels wide.
    pub(crate) fn thumb(&self, span: Span, width: f32, track: f32) -> Thumb {
        let track = track.max(0.0);
        let total = span.len();
        let visible = self.visible(width);
        let share = if total > 0.0 {
            (visible / total).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let w = (share * track).max(MIN_THUMB.min(track));
        let room = total - visible;
        let along = if room > 0.0 {
            ((self.start - span.lo) / room).clamp(0.0, 1.0)
        } else {
            0.0
        };
        Thumb {
            x: along * (track - w).max(0.0),
            w,
        }
    }

    /// The view start that puts a thumb `thumb_w` wide at `x` on the track:
    /// the inverse of [`Self::thumb`], for a thumb drag.
    pub(crate) fn start_for_thumb(
        &self,
        span: Span,
        width: f32,
        track: f32,
        thumb_w: f32,
        x: f32,
    ) -> f32 {
        let free = track - thumb_w;
        let room = span.len() - self.visible(width);
        if free <= 0.0 || room <= 0.0 {
            return span.lo;
        }
        span.lo + (x / free).clamp(0.0, 1.0) * room
    }

    /// A click on the track beside the thumb: one page (the view less a
    /// little overlap) toward the click, kept inside `span`.
    pub(crate) fn page(&mut self, span: Span, width: f32, forward: bool) {
        let step = self.visible(width) * (1.0 - PAGE_OVERLAP);
        self.start += if forward { step } else { -step };
        self.clamp_to(span, width);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tl(start: f32, scale: f32) -> Timeline {
        Timeline {
            start,
            scale,
            fps: 30,
        }
    }

    #[test]
    fn the_range_runs_past_the_end() {
        let t = tl(0.0, 100.0);
        let s = t.content_span(4.0);
        assert_eq!(s.hi, 6.0, "half the clip again past a long End");
        assert_eq!(t.content_span(1.0).hi, 1.0 + PAST_END_MIN);
        assert_eq!(
            t.content_span(0.0).hi,
            PAST_END_MIN,
            "an empty clip still scrolls"
        );
        assert!(
            (s.lo + LEAD / 100.0).abs() < 1e-6,
            "the room fit leaves before 0"
        );
    }

    #[test]
    fn the_scroll_span_holds_the_view() {
        // 1000 px at 100 px/s shows 10 s; a 2 s clip's range is 0..4.
        let t = tl(-0.24, 100.0);
        let s = t.scroll_span(2.0, 1000.0);
        assert!((s.hi - 9.76).abs() < 1e-4);
        assert_eq!(t.thumb(s, 1000.0, 500.0), Thumb { x: 0.0, w: 500.0 });
    }

    #[test]
    fn the_thumb_is_the_visible_share_at_the_view_start() {
        // Range -0.24..10 s, view 2 s wide (200 px at 100 px/s).
        let mut t = tl(-0.24, 100.0);
        let span = Span {
            lo: -0.24,
            hi: 9.76,
        };
        let at_start = t.thumb(span, 200.0, 500.0);
        assert!((at_start.w - 100.0).abs() < 1e-3, "2 of 10 s is a fifth");
        assert_eq!(at_start.x, 0.0);
        t.start = 7.76;
        let at_end = t.thumb(span, 200.0, 500.0);
        assert!(
            (at_end.x - 400.0).abs() < 1e-3,
            "the last page fills the track's end"
        );
        t.start = 3.76;
        assert!((t.thumb(span, 200.0, 500.0).x - 200.0).abs() < 1e-3);
    }

    #[test]
    fn a_tiny_share_keeps_a_grabbable_thumb() {
        let t = tl(0.0, 6000.0);
        let span = Span { lo: 0.0, hi: 600.0 };
        let thumb = t.thumb(span, 300.0, 400.0);
        assert_eq!(thumb.w, MIN_THUMB);
        let narrow = t.thumb(span, 300.0, 10.0);
        assert_eq!(narrow.w, 10.0, "never wider than the track");
    }

    #[test]
    fn dragging_the_thumb_inverts_its_placement() {
        let mut t = tl(0.0, 100.0);
        let span = Span { lo: 0.0, hi: 10.0 };
        let (width, track) = (200.0, 500.0);
        for x in [0.0, 37.5, 200.0, 400.0] {
            let w = t.thumb(span, width, track).w;
            t.start = t.start_for_thumb(span, width, track, w, x);
            assert!((t.thumb(span, width, track).x - x).abs() < 1e-3, "x {x}");
        }
        let w = t.thumb(span, width, track).w;
        assert_eq!(t.start_for_thumb(span, width, track, w, -50.0), 0.0);
        assert_eq!(t.start_for_thumb(span, width, track, w, 900.0), 8.0);
    }

    #[test]
    fn clamping_keeps_the_view_in_range() {
        let span = Span { lo: -0.2, hi: 6.0 };
        let mut t = tl(-3.0, 100.0);
        t.clamp_to(span, 200.0);
        assert_eq!(t.start, -0.2);
        t.start = 9.0;
        t.clamp_to(span, 200.0);
        assert_eq!(t.start, 4.0, "the last page ends at the range's end");
        t.start = 2.0;
        t.clamp_to(span, 2000.0);
        assert!(
            (t.start + 0.2).abs() < 1e-4,
            "a view as wide as the range starts at its left"
        );
    }

    #[test]
    fn zooming_out_stops_when_the_range_fits() {
        let span = Span { lo: 0.0, hi: 10.0 };
        let mut t = tl(0.0, 5.0);
        t.clamp_to(span, 200.0);
        assert_eq!(t.scale, 20.0, "the whole range just fills the view");
        t.scale = 80.0;
        t.clamp_to(span, 200.0);
        assert_eq!(t.scale, 80.0, "zooming in is left alone");
    }

    #[test]
    fn a_track_click_pages_with_overlap() {
        let span = Span { lo: 0.0, hi: 10.0 };
        let mut t = tl(0.0, 100.0);
        t.page(span, 200.0, true);
        assert!((t.start - 1.8).abs() < 1e-4);
        t.page(span, 200.0, false);
        assert!(t.start.abs() < 1e-4);
        t.page(span, 200.0, false);
        assert_eq!(t.start, 0.0, "paging stops at the range");
        t.start = 7.5;
        t.page(span, 200.0, true);
        assert_eq!(t.start, 8.0);
    }
}
