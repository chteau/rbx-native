use rbx_dom::{Content, Ref, Variant};

use super::*;

fn emitter(edit: impl FnOnce(&mut GuiEmitter)) -> GuiEmitter {
    let mut emitter = (*read(&Properties::new(), Ref::new(42))).clone();
    // Still particles that never die, unless a test says otherwise.
    emitter.rate = 0.0;
    emitter.speed = (0.0, 0.0);
    emitter.lifetime = (100.0, 100.0);
    edit(&mut emitter);
    emitter
}

const FRAME: FlatFrame = FlatFrame {
    center: [150.0, 150.0],
    size: [100.0, 100.0],
    rotation: 0.0,
    scale: 1.0,
};

fn run(emitter: &GuiEmitter, frame: FlatFrame, count: u32, seconds: f32) -> Vec<FlatParticle> {
    let mut sim = FlatSim::new(7);
    sim.emit(count);
    sim.step(emitter, frame, 0.0);
    let steps = (seconds * 100.0).round() as u32;
    for _ in 0..steps {
        sim.step(emitter, frame, 0.01);
    }
    sim.particles(emitter, frame).collect()
}

fn near(a: f32, b: f32, within: f32) -> bool {
    (a - b).abs() <= within
}

#[test]
fn a_fresh_emitter_reads_studios_defaults() {
    let fresh = read(&Properties::new(), Ref::new(42));
    assert_eq!(fresh.rate, 20.0);
    assert_eq!(fresh.lifetime, (5.0, 10.0));
    assert_eq!(fresh.speed, (64.0, 64.0));
    assert!(fresh.bounding_box);
    assert!(fresh.enabled && !fresh.locked);
    assert_eq!(fresh.size.keypoints[0].value, 32.0);
    assert_eq!(
        fresh.texture,
        AssetRef::parse("rbxasset://textures/particles/sparkles_main.dds").ok()
    );
    assert!(fresh.flipbook.blend_frames);
    assert_eq!(fresh.flipbook.frames(), 1);
}

#[test]
fn an_emptied_texture_draws_nothing() {
    let mut properties = Properties::new();
    properties.insert("TextureContent".into(), Variant::Content(Content::None));
    assert_eq!(read(&properties, Ref::new(42)).texture, None);
}

#[test]
fn time_scale_and_blend_are_clamped_like_studio() {
    let mut properties = Properties::new();
    properties.insert("TimeScale".into(), Variant::Float32(2.0));
    properties.insert("Blend".into(), Variant::Float32(-1.0));
    let read = read(&properties, Ref::new(42));
    assert_eq!((read.time_scale, read.blend), (1.0, 0.0));
}

#[test]
fn a_point_emitter_spawns_at_the_centre_moved_by_its_offset() {
    let point = emitter(|e| {
        e.bounding_box = false;
        e.offset = [0.5, 0.25];
    });
    let [particle] = run(&point, FRAME, 1, 0.0)[..] else {
        panic!("one particle");
    };
    assert_eq!(particle.center, [200.0, 175.0]);
}

#[test]
fn a_bounding_box_fills_the_parent_and_spread_grows_it() {
    let boxed = emitter(|_| {});
    for particle in run(&boxed, FRAME, 300, 0.0) {
        assert!((100.0..=200.0).contains(&particle.center[0]));
        assert!((100.0..=200.0).contains(&particle.center[1]));
    }
    let wide = emitter(|e| e.spread = [1.0, 0.0]);
    let xs: Vec<f32> = run(&wide, FRAME, 300, 0.0)
        .iter()
        .map(|p| p.center[0])
        .collect();
    assert!(xs.iter().all(|x| (50.0..=250.0).contains(x)));
    assert!(xs.iter().any(|x| *x < 90.0) && xs.iter().any(|x| *x > 210.0));
}

#[test]
fn a_point_emitters_spread_alone_is_the_area() {
    let point = emitter(|e| {
        e.bounding_box = false;
        e.spread = [1.0, 1.0];
    });
    for particle in run(&point, FRAME, 200, 0.0) {
        assert!((100.0..=200.0).contains(&particle.center[0]));
    }
}

#[test]
fn angle_ninety_is_right_speed_is_pixels_and_acceleration_is_down() {
    let thrown = emitter(|e| {
        e.bounding_box = false;
        e.speed = (100.0, 100.0);
        e.angle = 90.0;
        e.acceleration = [0.0, 100.0];
    });
    let [particle] = run(&thrown, FRAME, 1, 1.0)[..] else {
        panic!("one particle");
    };
    assert!(near(particle.center[0], 250.0, 0.5), "{particle:?}");
    assert!(near(particle.center[1], 200.0, 1.0), "{particle:?}");
}

#[test]
fn drag_halves_the_speed_drag_times_a_second() {
    for drag in [1.0, 2.0] {
        let dragged = emitter(|e| {
            e.bounding_box = false;
            e.speed = (100.0, 100.0);
            e.angle = 90.0;
            e.drag = drag;
        });
        let [particle] = run(&dragged, FRAME, 1, 1.0)[..] else {
            panic!("one particle");
        };
        // The integral of 100 * 0.5^(drag t) over one second.
        let expected = 100.0 * (1.0 - 0.5f32.powf(drag)) / (drag * 2f32.ln());
        assert!(
            near(particle.center[0] - 150.0, expected, 1.0),
            "{particle:?}"
        );
    }
}

#[test]
fn a_locked_particle_rides_along_and_a_free_one_stays() {
    let moved = FlatFrame {
        center: [300.0, 150.0],
        ..FRAME
    };
    for locked in [true, false] {
        let point = emitter(|e| {
            e.bounding_box = false;
            e.locked = locked;
        });
        let mut sim = FlatSim::new(1);
        sim.emit(1);
        sim.step(&point, FRAME, 0.0);
        let particle = sim.particles(&point, moved).next().expect("one particle");
        let expected = match locked {
            true => 300.0,
            false => 150.0,
        };
        assert_eq!(particle.center[0], expected);
    }
}

#[test]
fn a_turned_parent_turns_the_area_the_angle_and_the_particle() {
    let turned = FlatFrame {
        rotation: 90.0,
        ..FRAME
    };
    let point = emitter(|e| {
        e.bounding_box = false;
        e.offset = [0.0, -0.5];
        e.rotation = (10.0, 10.0);
    });
    let [particle] = run(&point, turned, 1, 0.0)[..] else {
        panic!("one particle");
    };
    // The top edge's middle, turned a quarter clockwise, is the right edge's.
    assert!(near(particle.center[0], 200.0, 1e-3) && near(particle.center[1], 150.0, 1e-3));
    assert_eq!(particle.rotation, 100.0);
}

#[test]
fn size_is_the_half_side_scaled_and_squash_trades_width_for_height() {
    let squashed = emitter(|e| {
        e.bounding_box = false;
        e.size = crate::scene::props::number_sequence(&[(0.0, 15.0), (1.0, 15.0)]);
        e.squash = crate::scene::props::number_sequence(&[(0.0, 1.0), (1.0, 1.0)]);
    });
    let scaled = FlatFrame {
        scale: 2.0,
        ..FRAME
    };
    let [particle] = run(&squashed, scaled, 1, 0.0)[..] else {
        panic!("one particle");
    };
    assert_eq!(particle.extent, [30.0, 120.0]);
}

#[test]
fn time_scale_zero_freezes_and_rate_spawns_per_second() {
    let frozen = emitter(|e| {
        e.bounding_box = false;
        e.speed = (100.0, 100.0);
        e.time_scale = 0.0;
    });
    let [particle] = run(&frozen, FRAME, 1, 1.0)[..] else {
        panic!("one particle");
    };
    assert_eq!(particle.center, [150.0, 150.0]);

    let steady = emitter(|e| e.rate = 20.0);
    assert!((19..=20).contains(&run(&steady, FRAME, 0, 1.0).len()));
    let disabled = emitter(|e| {
        e.rate = 20.0;
        e.enabled = false;
    });
    assert_eq!(run(&disabled, FRAME, 5, 1.0).len(), 5);
}

#[test]
fn clear_drops_every_particle() {
    let point = emitter(|_| {});
    let mut sim = FlatSim::new(3);
    sim.emit(10);
    sim.step(&point, FRAME, 0.0);
    assert!(!sim.is_empty());
    sim.clear();
    assert!(sim.is_empty());
}

#[test]
fn flipbook_frames_follow_the_modes_measured_in_studio() {
    let book = |mode, fps: f32, life: f32, blend| {
        emitter(|e| {
            e.bounding_box = false;
            e.lifetime = (life, life);
            e.flipbook = Flipbook {
                columns: 2,
                rows: 2,
                mode,
                framerate: (fps, fps),
                start_random: false,
                blend_frames: blend,
            };
        })
    };
    let frame = |emitter: &GuiEmitter| run(emitter, FRAME, 1, 2.4)[0].frames;
    // Studio after 2.4 s: blue, green, white, red (frames 2, 1, 3, 0).
    assert_eq!(frame(&book(FlipbookMode::Loop, 1.0, 100.0, false)).0, 2);
    assert_eq!(frame(&book(FlipbookMode::OneShot, 1.0, 8.0, false)).0, 1);
    assert_eq!(frame(&book(FlipbookMode::PingPong, 1.5, 100.0, false)).0, 3);
    assert_eq!(frame(&book(FlipbookMode::Loop, 0.0, 100.0, false)).0, 0);
    let (current, next, between) = frame(&book(FlipbookMode::Loop, 1.0, 100.0, true));
    assert_eq!((current, next), (2, 3));
    assert!(near(between, 0.4, 0.02));
}
