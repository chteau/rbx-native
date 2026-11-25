//! `ParticleEmitter2D:Emit(count)` and `Clear()`. Neither touches the DOM:
//! they act on the particles a renderer is running, so a call is queued here
//! for the host to hand on (`rbx_viewer::gui_particle_burst`).

use std::sync::Mutex;

use rbx_dom::Ref;

pub(crate) const CLASS: &str = "ParticleEmitter2D";
/// `Emit`'s count when none is given.
pub(crate) const DEFAULT_COUNT: u32 = 16;

static BURSTS: Mutex<Vec<(Ref, Option<u32>)>> = Mutex::new(Vec::new());

pub(crate) fn push(emitter: Ref, burst: Option<u32>) {
    if let Ok(mut queue) = BURSTS.lock() {
        queue.push((emitter, burst));
    }
}

/// Every `Emit(count)` (`Some`) and `Clear()` (`None`) scripts made since the
/// last call, oldest first.
pub fn take_particle_bursts() -> Vec<(Ref, Option<u32>)> {
    BURSTS
        .lock()
        .map(|mut queue| std::mem::take(&mut *queue))
        .unwrap_or_default()
}
