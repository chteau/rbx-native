//! The Audio API on a client: a LocalScript's AudioPlayer -> AudioFader ->
//! AudioDeviceOutput graph plays at the fader's level, and an analyzer on
//! the fader reads it. Its own test binary: it points the asset cache at a
//! folder of its own, holding the tone the player plays.

use rbx_dom::{Ref, Variant};
use rbx_sim::{conformance, Peer, Session};

const ASSET: u64 = 990_001;
const AMPLITUDE: f32 = 0.5;

/// One second of 440 Hz, mono 16-bit at 48 kHz.
fn tone() -> Vec<u8> {
    let samples: Vec<i16> = (0..48_000)
        .map(|i| {
            ((i as f32 / 48_000.0 * 440.0 * std::f32::consts::TAU).sin() * AMPLITUDE * 32767.0)
                as i16
        })
        .collect();
    let data = (samples.len() * 2) as u32;
    let mut wav = Vec::new();
    for chunk in [
        &b"RIFF"[..],
        &(36 + data).to_le_bytes(),
        b"WAVEfmt ",
        &16u32.to_le_bytes(),
    ] {
        wav.extend_from_slice(chunk);
    }
    for field in [1u16, 1] {
        wav.extend_from_slice(&field.to_le_bytes());
    }
    for field in [48_000u32, 96_000] {
        wav.extend_from_slice(&field.to_le_bytes());
    }
    for field in [2u16, 16] {
        wav.extend_from_slice(&field.to_le_bytes());
    }
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data.to_le_bytes());
    wav.extend(samples.iter().flat_map(|s| s.to_le_bytes()));
    wav
}

const CLIENT: &str = r#"
local function new(class, props)
	local instance = Instance.new(class)
	for key, value in props do
		instance[key] = value
	end
	instance.Parent = workspace
	return instance
end
local player = new("AudioPlayer", { Asset = "rbxassetid://990001", Looping = true })
local fader = new("AudioFader", { Volume = 0.5 })
local output = new("AudioDeviceOutput", {})
local analyzer = new("AudioAnalyzer", {})
new("Wire", { SourceInstance = player, TargetInstance = fader })
new("Wire", { SourceInstance = fader, TargetInstance = output })
new("Wire", { SourceInstance = fader, TargetInstance = analyzer })
while not player.IsReady do
	task.wait()
end
player:Play()
task.wait(1)
print(string.format("rms %.3f", analyzer.RmsLevel))
"#;

#[test]
fn a_client_graph_plays_at_the_fader_level_and_the_analyzer_reads_it() {
    let cache = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("audio-graph-cache");
    std::fs::create_dir_all(cache.join("rbx-native/assets/ids")).unwrap();
    std::fs::write(cache.join(format!("rbx-native/assets/ids/{ASSET}")), tone()).unwrap();
    // Alone in this binary: nothing else reads the environment meanwhile.
    std::env::set_var("XDG_CACHE_HOME", &cache);

    let mut dom = conformance::bare_place();
    let starter_player = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == "StarterPlayer"))
        .unwrap();
    let scripts: Ref = dom.get(starter_player).unwrap().children()[0];
    let local = dom.new_instance("LocalScript", "Graph", Some(scripts));
    let _ = dom.set_property(local, "Source", Variant::String(CLIENT.into()));
    dom.take_changes();
    let mut session = Session::new(dom).expect("the place runs");
    session.start();
    session.join("Player1", -1, None).expect("joins");

    let mut printed = Vec::new();
    for _ in 0..150 {
        session.step();
        for (peer, output) in session.take_output() {
            printed.extend(output.lines().iter().map(|l| (peer, l.clone())));
        }
    }
    let expected = AMPLITUDE * 0.5 / 2f32.sqrt();
    let read: f32 = printed
        .iter()
        .find_map(|(peer, line)| {
            line.strip_prefix("rms ")
                .filter(|_| *peer == Peer::Client(0))
        })
        .unwrap_or_else(|| panic!("no reading in {printed:#?}"))
        .parse()
        .unwrap();
    assert!(
        (read - expected).abs() < 0.005,
        "the analyzer read {read}, not {expected}"
    );

    let played = session.sim(Peer::Client(0)).unwrap().take_audio();
    let last = &played[played.len() - 12_000..];
    let rms = (last.iter().map(|f| f[0] * f[0]).sum::<f32>() / last.len() as f32).sqrt();
    assert!(
        (rms - expected).abs() < 0.005,
        "rendered at {rms}, not {expected}"
    );
}
