//! `cargo run -p rbx_autorig --example fit_report -- <mesh> [out.svg]`:
//! fits the skeleton to a mesh file, prints the orientation, scale, joints
//! and warnings, and optionally draws the fitted mesh's front and side
//! silhouettes with the joints on them, to check a fit without the editor.

use std::fmt::Write as _;

use rbx_autorig::{Joint, Part};

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: fit_report <mesh> [out.svg]");
        std::process::exit(2);
    };
    let mesh = rbx_autorig::read(std::path::Path::new(&path)).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1)
    });
    println!(
        "{path}: {} vertices, {} triangles, up hint {:?}, front hint {:?}",
        mesh.positions.len(),
        mesh.triangles.len(),
        mesh.up_hint,
        mesh.front_hint
    );
    let fit = match rbx_autorig::fit(&mesh, 5.2) {
        Ok(fit) => fit,
        Err(e) => {
            println!("fit failed: {e}");
            std::process::exit(1)
        }
    };
    println!(
        "orientation up {:?} front {:?}, scale {:.5}",
        fit.orientation.up, fit.orientation.front, fit.scale
    );
    for joint in Joint::ALL {
        let p = fit.joint(joint);
        println!(
            "  {:<14} {:>7.3} {:>7.3} {:>7.3}",
            joint.name(),
            p.x,
            p.y,
            p.z
        );
    }
    let e = &fit.extremities;
    println!(
        "  extremities head {:.2?} hands {:.2?} {:.2?} feet {:.2?} {:.2?}",
        e.head, e.left_hand, e.right_hand, e.left_foot, e.right_foot
    );
    for w in &fit.warnings {
        println!("  warning: {w}");
    }
    match rbx_autorig::split(&mesh, &fit) {
        Ok(body) => {
            for part in Part::ALL {
                let p = body.part(part);
                println!(
                    "  {:<14} {:>6} tris, size {:.2?}",
                    part.name(),
                    p.mesh.triangles.len(),
                    p.size
                );
            }
            for w in &body.warnings {
                println!("  split warning: {w}");
            }
        }
        Err(e) => println!("split failed: {e}"),
    }
    if let Some(out) = args.next() {
        std::fs::write(&out, svg(&mesh, &fit)).expect("write svg");
    }
}

/// Front (x, y) and side (z, y) views, 400 px per 5.2 studs.
fn svg(mesh: &rbx_autorig::Mesh, fit: &rbx_autorig::Fit) -> String {
    let s = 400. / 5.2;
    let (w, h) = (1350., 460.);
    let mut out = format!(
        "<svg xmlns='http://www.w3.org/2000/svg' width='{w}' height='{h}'><rect width='100%' height='100%' fill='white'/>"
    );
    type View = (
        f32,
        fn(glam::Vec3) -> f32,
        fn(glam::Vec3) -> f32,
        &'static str,
    );
    let views: [View; 3] = [
        (
            225.,
            |p| p.x,
            |p| 430. - p.y * 400. / 5.2,
            "front (+x right, red = left)",
        ),
        (
            675.,
            |p| p.z,
            |p| 430. - p.y * 400. / 5.2,
            "side (+z right, front is -z)",
        ),
        (
            1125.,
            |p| p.x,
            |p| 230. + p.z * 400. / 5.2,
            "top (+x right, front is up)",
        ),
    ];
    for (ox, axis, vertical, title) in views {
        let map = |p: glam::Vec3| (ox + axis(p) * s, vertical(p));
        for t in mesh.triangles.iter().step_by(1) {
            let [a, b, c] = t.map(|i| map(fit.to_roblox(mesh.positions[i as usize])));
            let _ = write!(
                out,
                "<polygon points='{:.1},{:.1} {:.1},{:.1} {:.1},{:.1}' fill='#bbb' fill-opacity='0.08' stroke='none'/>",
                a.0, a.1, b.0, b.1, c.0, c.1
            );
        }
        for part in Part::ALL {
            if let Some(parent) = part.parent() {
                let (a, b) = (map(fit.joint(part.joint())), map(fit.joint(parent.joint())));
                let _ = write!(
                    out,
                    "<line x1='{:.1}' y1='{:.1}' x2='{:.1}' y2='{:.1}' stroke='blue' stroke-width='2'/>",
                    a.0, a.1, b.0, b.1
                );
            }
        }
        for joint in Joint::ALL {
            let (x, y) = map(fit.joint(joint));
            let colour = match joint.side() {
                Some(rbx_autorig::Side::Left) => "red",
                Some(rbx_autorig::Side::Right) => "green",
                None => "black",
            };
            let _ = write!(
                out,
                "<circle cx='{x:.1}' cy='{y:.1}' r='4' fill='{colour}'/>"
            );
        }
        let _ = write!(
            out,
            "<text x='{}' y='20' font-size='14'>{}</text>",
            ox - 60.,
            title
        );
        let e = &fit.extremities;
        for p in [e.head, e.left_hand, e.right_hand, e.left_foot, e.right_foot] {
            let (x, y) = map(p);
            let _ = write!(out, "<circle cx='{x:.1}' cy='{y:.1}' r='5' fill='orange'/>");
        }
    }
    out.push_str("</svg>");
    out
}
