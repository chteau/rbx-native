//! Videos and text-to-speech on a client: a `VideoFrame`'s clock, events,
//! picture and sound over a real WebM clip (tests/data/clip.webm: one
//! second, 64x48 VP8 at 10 fps, red then blue, with an Opus tone), and an
//! `AudioTextToSpeech` that speaks.

use std::sync::Once;

use rbx_dom::{Content, Ref, Variant};
use rbx_sim::{conformance, Peer, Session};

const VIDEO: u64 = 990_001;

/// Every test in this binary reads assets from one cache holding the clip.
fn cache() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let cache = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("media-cache");
        std::fs::create_dir_all(cache.join("rbx-native/assets/ids")).unwrap();
        std::fs::write(
            cache.join(format!("rbx-native/assets/ids/{VIDEO}")),
            include_bytes!("data/clip.webm"),
        )
        .unwrap();
        std::env::set_var("XDG_CACHE_HOME", &cache);
    });
}

/// A place whose ScreenGui runs `source` as a LocalScript, joined by one
/// player.
fn playing(source: &str) -> Session {
    cache();
    let mut dom = conformance::bare_place();
    let starter = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == "StarterGui"))
        .unwrap();
    let screen = dom.new_instance("ScreenGui", "Screen", Some(starter));
    let script = dom.new_instance("LocalScript", "Probe", Some(screen));
    let _ = dom.set_property(script, "Source", Variant::String(source.into()));
    dom.take_changes();
    let mut session = Session::new(dom).expect("the place runs");
    session.start();
    session.join("Player1", 1, None).expect("joins");
    session
}

/// What the client printed, less the default character's animations
/// failing to load (there's no fetcher here).
fn output(session: &mut Session) -> Vec<String> {
    let mut lines = Vec::new();
    for (peer, out) in session.take_output() {
        if peer == Peer::Client(0) {
            lines.extend(
                out.lines()
                    .iter()
                    .filter(|l| !l.starts_with("Failed to load animation"))
                    .cloned(),
            );
        }
    }
    lines
}

fn find(session: &Session, class: &str) -> Ref {
    let sim = session.sim(Peer::Client(0)).unwrap();
    let dom = sim.dom();
    let mut stack = dom.root_refs().to_vec();
    while let Some(r) = stack.pop() {
        let instance = dom.get(r).unwrap();
        if instance.class() == class {
            return r;
        }
        stack.extend(instance.children());
    }
    panic!("no {class}");
}

#[test]
fn a_video_frame_plays_ends_and_loops_on_its_clock() {
    let mut session = playing(&format!(
        r#"
local frame = Instance.new("VideoFrame")
frame.Size = UDim2.fromOffset(32, 24)
frame.Video = "rbxassetid://{VIDEO}"
frame.Parent = script.Parent
frame.Ended:Connect(function() print("ended", frame.Playing, frame.TimePosition) end)
frame.DidLoop:Once(function() print("looped", frame.Playing) end)
if not frame.IsLoaded then frame.Loaded:Wait() end
print("loaded", frame.TimeLength > 0.95 and frame.TimeLength < 1.05, frame.Resolution)
frame:Play()
task.wait(0.4)
print("midway", frame.Playing, frame.TimePosition > 0.3 and frame.TimePosition < 0.6)
frame.Ended:Wait()
task.wait(0.1)
frame.Looped = true
frame.TimePosition = 0.8
frame:Play()
frame.DidLoop:Wait()
task.wait(0.1)
print("after loop", frame.Playing, frame.TimePosition < 0.5)
"#
    ));
    session.run_for(0.6);
    // Halfway through, the client draws the frame's picture and plays its
    // sound.
    let frame = find(&session, "VideoFrame");
    let sim = session.sim(Peer::Client(0)).unwrap();
    assert!(sim
        .voices()
        .iter()
        .any(|v| v.sound == frame && v.clip.rate == 48_000));
    let bound = sim
        .dom()
        .get(frame)
        .unwrap()
        .properties()
        .get(rbx_dom::editable::VIDEO_PICTURE)
        .cloned();
    let Some(Variant::Content(Content::Object(object))) = bound else {
        panic!("no picture bound: {bound:?}");
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Some(rbx_dom::editable::Editable::Image(p)) = rbx_dom::editable::get(object) {
            let p = rbx_dom::editable::lock(&p).clone();
            if p.version > 0 {
                // Drawn at the frame's 32x24, not the video's 64x48.
                assert_eq!((p.width, p.height), (32, 24));
                break;
            }
        }
        assert!(std::time::Instant::now() < deadline, "no frame decoded");
        std::thread::yield_now();
    }
    session.run_for(2.0);
    assert_eq!(
        output(&mut session),
        [
            "loaded true 64, 48",
            "midway true true",
            "ended false 0",
            "looped true",
            "after loop true true",
        ]
    );
}

#[test]
fn text_to_speech_speaks_through_the_graph() {
    let mut session = playing(
        r#"
local tts = Instance.new("AudioTextToSpeech")
tts.Text = "Hello from the local run"
tts.VoiceId = "1"
tts.Parent = workspace
local out = Instance.new("AudioDeviceOutput")
out.Parent = workspace
local wire = Instance.new("Wire")
wire.SourceInstance = tts
wire.TargetInstance = out
wire.Parent = workspace
print("load", tts:LoadAsync().Name, tts.IsLoaded, tts.TimeLength > 0.5)
local length, took = tts.TimeLength, 0
local clock = game:GetService("RunService").Heartbeat:Connect(function(dt) took += dt end)
tts:Play()
tts.Ended:Wait()
clock:Disconnect()
print("ended", tts.IsPlaying, math.abs(took - length) < 0.2)
tts.Text = ""
print("empty", tts:LoadAsync().Name, tts.IsLoaded)
"#,
    );
    let mut heard = 0f32;
    for _ in 0..240 {
        session.step();
        let audio = session.sim(Peer::Client(0)).unwrap().take_audio();
        heard = audio.iter().fold(heard, |m, f| m.max(f[0].abs()));
    }
    assert_eq!(
        output(&mut session),
        [
            "load Success true true",
            "ended false true",
            "empty Failure false"
        ]
    );
    assert!(heard > 0.02, "the speech rendered silent ({heard})");
}
