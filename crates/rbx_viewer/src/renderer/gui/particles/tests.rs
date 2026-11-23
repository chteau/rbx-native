use std::sync::Arc;

use super::*;
use crate::scene::{FlatFrame, GuiEmitter};

fn particle(rotation: f32, frames: (u32, u32, f32)) -> FlatParticle {
    FlatParticle {
        center: [100.0, 50.0],
        extent: [20.0, 40.0],
        rotation,
        color: [1.0, 0.5, 0.25],
        alpha: 0.5,
        frames,
    }
}

#[test]
fn a_particle_is_two_triangles_around_its_centre() {
    let mut vertices = Vec::new();
    quad(&particle(0.0, (0, 0, 0.0)), 2.0, 1.0, (1, 1), &mut vertices);
    assert_eq!(vertices.len(), 6);
    assert_eq!(vertices[0].position, [90.0, 30.0]);
    assert_eq!(vertices[2].position, [110.0, 70.0]);
    assert_eq!((vertices[0].uv, vertices[2].uv), ([0.0, 0.0], [1.0, 1.0]));
    assert_eq!((vertices[0].brightness, vertices[0].blend), (2.0, 1.0));
}

#[test]
fn a_turn_is_clockwise_on_screen() {
    let mut vertices = Vec::new();
    quad(
        &particle(90.0, (0, 0, 0.0)),
        1.0,
        0.0,
        (1, 1),
        &mut vertices,
    );
    // The top-left corner, a quarter turn clockwise, is the top-right one
    // of the turned box: right of the centre by its half height.
    let [x, y] = vertices[0].position;
    assert!(
        (x - 120.0).abs() < 1e-4 && (y - 40.0).abs() < 1e-4,
        "{x} {y}"
    );
}

#[test]
fn flipbook_frames_pick_their_grid_cells_row_by_row() {
    let mut vertices = Vec::new();
    quad(
        &particle(0.0, (1, 2, 0.25)),
        1.0,
        0.0,
        (2, 2),
        &mut vertices,
    );
    assert_eq!(vertices[0].uv, [0.5, 0.0]);
    assert_eq!(vertices[0].uv_next, [0.0, 0.5]);
    assert_eq!(vertices[2].uv, [1.0, 0.5]);
    assert_eq!(vertices[0].between, 0.25);
}

#[test]
fn a_script_burst_spawns_on_the_next_frame_and_clear_empties() {
    let referent = Ref::new(42);
    let texture = AssetRef::parse("rbxasset://textures/particles/sparkles_main.dds").unwrap();
    let emitter = GuiEmitter {
        rate: 0.0,
        texture: Some(texture.clone()),
        ..(*crate::scene::read_gui_emitter(&Default::default(), referent)).clone()
    };
    let slots = [GuiParticles {
        emitter: Arc::new(emitter),
        frame: FlatFrame {
            center: [50.0, 50.0],
            size: [100.0, 100.0],
            rotation: 0.0,
            scale: 1.0,
        },
        clip: None,
    }];
    let slot_of = HashMap::from([(
        texture,
        Slot {
            linear: 7,
            nearest: 8,
            size: [128.0, 128.0],
        },
    )]);
    let mut sims = Sims::new();
    sims.tick();
    let (vertices, _) = sims.build(&slots, &slot_of);
    assert!(vertices.is_empty());

    gui_particle_burst(referent, Some(3));
    sims.tick();
    let (vertices, ranges) = sims.build(&slots, &slot_of);
    assert_eq!(vertices.len(), 18);
    assert_eq!(ranges, vec![Some((7, 0..18))]);
    assert!(sims.moving());

    gui_particle_burst(referent, None);
    sims.tick();
    let (vertices, ranges) = sims.build(&slots, &slot_of);
    assert!(vertices.is_empty());
    assert_eq!(ranges, vec![None]);
}
