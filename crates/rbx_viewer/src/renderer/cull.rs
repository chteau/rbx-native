//! The main pass's own visibility test, and turning any per-instance
//! visibility test into the contiguous index ranges a draw call can skip.
//!
//! An instance buffer's order never changes just because the camera moved —
//! `shaped::Shaped` and `shadow::casters` both keep a fixed offset per
//! instance so a single Properties edit can patch straight into the buffer
//! (see their own `patch`) — so culling has to omit whole runs of the
//! *existing* order rather than reorder it.

use std::ops::Range;

use glam::Vec3;

use crate::camera::Frustum;

/// The main pass's own visibility test: the camera's tight view frustum plus
/// its quality level's own render distance.
///
/// Deliberately not what the shadow pass culls its casters against (see
/// `super::shadow::fit::Fit::visible`) — a caster just outside this test can
/// still need to land a shadow inside it, which is exactly the regression
/// this split exists to avoid.
pub(super) struct MainCull<'a> {
    frustum: &'a Frustum,
    eye: Vec3,
    max_distance: f32,
}

impl<'a> MainCull<'a> {
    pub(super) fn new(frustum: &'a Frustum, eye: Vec3, max_distance: f32) -> Self {
        MainCull {
            frustum,
            eye,
            max_distance,
        }
    }

    /// Whether a bounding sphere is worth drawing this frame: within the
    /// frustum, and not entirely past the render distance. An infinite
    /// `max_distance` (the top quality band) skips the distance half of the
    /// test exactly like the shader-side fade it stands in for
    /// (`render_distance_visible` in atmosphere.wgsl) does for that same
    /// value — nothing here culls anything that fade would still show.
    pub(super) fn visible(&self, center: Vec3, radius: f32) -> bool {
        if self.max_distance.is_finite() && self.eye.distance(center) - radius >= self.max_distance
        {
            return false;
        }
        self.frustum.visible(center, radius)
    }
}

/// The maximal runs of `visible` over `0..len`, in the order they already sit
/// in. Empty when nothing is visible; one range spanning everything when it
/// all is. A run of any length costs one draw call; the worst case
/// (visibility alternating every instance) costs as many draw calls as there
/// are visible instances, exactly like drawing them one at a time would, but
/// never more than that.
pub(super) fn visible_runs(len: u32, mut visible: impl FnMut(u32) -> bool) -> Vec<Range<u32>> {
    let mut runs = Vec::new();
    let mut start: Option<u32> = None;
    for index in 0..len {
        if visible(index) {
            start.get_or_insert(index);
        } else if let Some(from) = start.take() {
            runs.push(from..index);
        }
    }
    if let Some(from) = start {
        runs.push(from..len);
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_visible_is_no_runs_at_all() {
        assert_eq!(visible_runs(4, |_| false), Vec::<Range<u32>>::new());
    }

    #[test]
    fn everything_visible_is_one_run() {
        assert_eq!(visible_runs(4, |_| true), vec![0..4]);
    }

    #[test]
    fn alternating_visibility_splits_into_single_instance_runs() {
        assert_eq!(visible_runs(4, |i| i % 2 == 0), vec![0..1, 2..3]);
    }

    #[test]
    fn a_visible_stretch_in_the_middle_is_one_run() {
        assert_eq!(visible_runs(5, |i| (1..4).contains(&i)), vec![1..4]);
    }

    #[test]
    fn zero_length_has_no_runs() {
        assert_eq!(visible_runs(0, |_| true), Vec::<Range<u32>>::new());
    }
}
