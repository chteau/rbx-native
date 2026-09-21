//! What the mesh-edge probe costs on real places, for the budget its
//! module doc states: `RBX_MESH_COST=<place>[:<place>…]`, run with
//! `--release --ignored`.

use std::time::{Duration, Instant};

use glam::Vec3;
use rbx_reflection::ReflectionDatabase;
use rbx_viewer::pick::{self, PartSurface, Ray, Solid};

use super::under;

/// The mesh-edge probe's cost on a real place: every `MeshPart`, aimed at
/// from fourteen directions, timed per target frame.
#[test]
#[ignore = "needs RBX_MESH_COST=<place> and a GPU; run in release"]
fn mesh_probe_cost() {
    let Some(paths) = std::env::var_os("RBX_MESH_COST") else {
        return;
    };
    for path in std::env::split_paths(&paths) {
        println!("{}", path.display());
        cost_in(&path);
    }
}

fn cost_in(path: &std::path::Path) {
    let dom = rbx_viewer::read_place(path).unwrap();
    let mut headless = rbx_viewer::Headless::load(path, false).unwrap();
    // Until nothing more lands for ten seconds: an asset the cache does not
    // hold may never arrive here.
    let mut quiet = Instant::now();
    let started = Instant::now();
    while headless.assets_in_flight() > 0 && quiet.elapsed() < Duration::from_secs(10) {
        if headless.swap_assets() {
            quiet = Instant::now();
        }
    }
    headless.swap_assets();
    println!(
        "settled in {:?}, {} assets still in flight",
        started.elapsed(),
        headless.assets_in_flight()
    );
    let meshes = headless.pick_meshes();
    let database = ReflectionDatabase::embedded();
    let directions: Vec<Vec3> = [
        Vec3::X,
        Vec3::NEG_X,
        Vec3::Y,
        Vec3::NEG_Y,
        Vec3::Z,
        Vec3::NEG_Z,
    ]
    .into_iter()
    .chain([-1.0, 1.0].into_iter().flat_map(|x| {
        [-1.0, 1.0]
            .into_iter()
            .flat_map(move |y| [-1.0, 1.0].map(|z| Vec3::new(x, y, z)))
    }))
    .map(Vec3::normalize)
    .collect();
    let mut costs = Vec::new();
    for referent in pick::drawable_parts(&dom, &database) {
        let Some(part) = PartSurface::read(&dom, &database, &meshes, referent) else {
            continue;
        };
        if part.solid != Solid::Mesh {
            continue;
        }
        let centre = part.model.w_axis.truncate();
        let reach =
            part.model.x_axis.length() + part.model.y_axis.length() + part.model.z_axis.length();
        let mut worst = Duration::ZERO;
        let mut total = Duration::ZERO;
        let mut hits = 0;
        let mut one = Duration::ZERO;
        for &direction in &directions {
            let ray = Ray::new(centre - direction * reach * 2.0, direction);
            let timed = Instant::now();
            let found = under(&part, ray, 1.0);
            let took = timed.elapsed();
            let timed = Instant::now();
            std::hint::black_box(part.raycast(ray));
            one = one.max(timed.elapsed());
            if found.is_some() {
                hits += 1;
                total += took;
                worst = worst.max(took);
            }
        }
        if hits > 0 {
            let name = dom
                .get(referent)
                .map(|i| i.name().to_owned())
                .unwrap_or_default();
            costs.push((worst, total / hits, one, name));
        }
    }
    println!("{} mesh parts hit", costs.len());
    costs.sort_by_key(|cost| std::cmp::Reverse(cost.2));
    for (worst, mean, one, name) in costs.iter().take(5) {
        println!("  frame worst {worst:?} mean {mean:?}; one raycast {one:?}: {name}");
    }
}
