//! Sound playback state (there's no audio): `Play`, `Stop`, `Pause` and
//! `Resume` set `Playing` (and `TimePosition` back to 0 for `Play` and
//! `Stop`), `IsPlaying` reads it, and the matching signal fires. A client
//! playing or stopping a sound tells the server, which everyone then
//! sees, unless `SoundService.RespectFilteringEnabled` is on
//! (ClientReplication golden: `played` true on the server; a `Volume` the
//! client writes stays its own).

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_lua::server::{Arg, CallReply};
use rbx_reflection::ReflectionDatabase;

pub(crate) fn is_method(class: &str, key: &str) -> bool {
    class == "Sound" && matches!(key, "Play" | "Stop" | "Pause" | "Resume")
}

pub(crate) fn is_signal(class: &str, key: &str) -> bool {
    class == "Sound" && matches!(key, "Played" | "Stopped" | "Paused" | "Resumed" | "Ended")
}

/// `Sound.IsPlaying`.
pub(crate) fn property(
    dom: &WeakDom,
    db: &ReflectionDatabase,
    referent: Ref,
    class: &str,
    key: &str,
) -> Option<Arg> {
    (class == "Sound" && key == "IsPlaying")
        .then(|| Arg::Bool(crate::humanoid::flag(dom, db, referent, "Playing")))
}

/// Whether a client's playback reaches the server.
pub(crate) fn replicates(dom: &WeakDom, db: &ReflectionDatabase) -> bool {
    let service = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == "SoundService"));
    !service.is_some_and(|s| crate::humanoid::flag(dom, db, s, "RespectFilteringEnabled"))
}

/// `Sound:Play()` and the rest: the new `Playing`, and the reply firing
/// the signal.
pub(crate) fn call(dom: &mut WeakDom, referent: Ref, method: &str) -> (bool, CallReply) {
    let (playing, rewind, signal) = match method {
        "Play" => (true, true, "Played"),
        "Stop" => (false, true, "Stopped"),
        "Pause" => (false, false, "Paused"),
        _ => (true, false, "Resumed"),
    };
    set(dom, referent, playing, rewind);
    let id = match dom
        .get(referent)
        .and_then(|i| i.properties().get("SoundId"))
    {
        Some(Variant::Content(rbx_dom::Content::Uri(s))) | Some(Variant::String(s)) => s.clone(),
        _ => String::new(),
    };
    (
        playing,
        CallReply {
            fire: vec![(referent, signal.into(), vec![Arg::String(id)])],
            ..CallReply::default()
        },
    )
}

/// Writes the playback state, logged so it replicates.
pub(crate) fn set(dom: &mut WeakDom, referent: Ref, playing: bool, rewind: bool) {
    let _ = dom.set_property(referent, "Playing", Variant::Bool(playing));
    if rewind {
        let _ = dom.set_property(referent, "TimePosition", Variant::Float64(0.0));
    }
}
