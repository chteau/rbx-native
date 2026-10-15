//! Collaborators in the 3D view: a box in their colour around each part
//! they have selected, and a marker where their camera is.

use glam::{Mat4, Vec3};
use rbx_dom::WeakDom;
use rbx_reflection::ReflectionDatabase;
use rbx_viewer::{pick, Segment};

use super::model::{Collaborator, Status};
use crate::tokens::linear;

/// Line width of a selection box, in pixels.
pub(super) const BOX_WIDTH: f32 = 2.0;

/// The segments for everyone in `collaborators` with status `Sending`: the
/// 12 edges of each selected part's box (a model's parts each get one,
/// through `rbx_viewer::pick::selection` and `pick::part_model` from the
/// part's `CFrame` and `size`), drawn on top; and per camera a 12 px dot at
/// its position plus a 4-stud line along its look direction (−Z column of
/// the rotation) and a small 4-line frustum (1.5 stud deep, 1 × 0.6 stud
/// face). Colours are linear RGBA (`crate::tokens::linear` of the sRGB
/// colour), alpha 1.
pub(super) fn segments(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    collaborators: &[Collaborator],
) -> Vec<Segment> {
    let mut out = Vec::new();
    for who in collaborators {
        if who.status != Status::Sending {
            continue;
        }
        let (r, g, b) = who.color;
        let color = linear(gpui_kit::rgb(
            (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b),
        ));
        for selected in pick::selection(dom, database, &who.selection) {
            for &part in selected.parts() {
                let Some(model) = pick::model_of(dom, part) else {
                    continue;
                };
                box_edges(model, color, &mut out);
            }
        }
        if let Some(c) = who.camera {
            camera_marker(&c, color, &mut out);
        }
    }
    out
}

fn line(from: Vec3, to: Vec3, color: [f32; 4], width: f32) -> Segment {
    Segment {
        from,
        to,
        color,
        on_top: true,
        width,
    }
}

fn box_edges(model: Mat4, color: [f32; 4], out: &mut Vec<Segment>) {
    let corner = |i: usize| {
        let sign = |bit: usize| if i & bit != 0 { 0.5 } else { -0.5 };
        model.transform_point3(Vec3::new(sign(1), sign(2), sign(4)))
    };
    for i in 0..8usize {
        for bit in [1usize, 2, 4] {
            if i & bit == 0 {
                out.push(line(corner(i), corner(i | bit), color, BOX_WIDTH));
            }
        }
    }
}

fn camera_marker(c: &[f64; 12], color: [f32; 4], out: &mut Vec<Segment>) {
    let v = |i: usize| c[i] as f32;
    let at = Vec3::new(v(0), v(1), v(2));
    let right = Vec3::new(v(3), v(6), v(9));
    let up = Vec3::new(v(4), v(7), v(10));
    let look = -Vec3::new(v(5), v(8), v(11));
    out.push(line(at, at, color, 12.0));
    out.push(line(at, at + look * 4.0, color, BOX_WIDTH));
    let center = at + look * 1.5;
    for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        let corner = center + right * (0.5 * sx) + up * (0.3 * sy);
        out.push(line(at, corner, color, 1.0));
    }
}

#[cfg(test)]
mod tests {
    use rbx_dom::{CFrameData, Variant, Vector3Data};

    use super::*;

    fn who(
        status: Status,
        selection: Vec<rbx_dom::Ref>,
        camera: Option<[f64; 12]>,
    ) -> Collaborator {
        Collaborator {
            user_id: 1,
            name: "a".into(),
            color: (255, 0, 0),
            client: None,
            status,
            selection,
            unresolved: 0,
            script: None,
            camera,
            edits: Vec::new(),
        }
    }

    fn scene() -> (WeakDom, rbx_dom::Ref) {
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        let id = dom.new_instance("Part", "Part", Some(workspace));
        let cframe = CFrameData {
            position: Vector3Data {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        };
        dom.set_property(id, "CFrame", Variant::CFrame(cframe))
            .unwrap();
        dom.set_property(
            id,
            // The binary format's spelling, which the DOM keeps.
            "size",
            Variant::Vector3(Vector3Data {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            }),
        )
        .unwrap();
        (dom, id)
    }

    const CAMERA: [f64; 12] = [5.0, 6.0, 7.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

    #[test]
    fn selected_part_gives_twelve_edges() {
        let (dom, id) = scene();
        let db = ReflectionDatabase::shared();
        let out = segments(&dom, db, &[who(Status::Sending, vec![id], None)]);
        let want = linear(gpui_kit::rgb(0xff0000));
        assert_eq!(out.len(), 12);
        assert!(out.iter().all(|s| s.on_top && s.color == want));
    }

    #[test]
    fn camera_gives_dot_and_lines() {
        let (dom, _) = scene();
        let db = ReflectionDatabase::shared();
        let out = segments(&dom, db, &[who(Status::Sending, vec![], Some(CAMERA))]);
        assert_eq!(out.len(), 6);
        assert_eq!(out[0].from, Vec3::new(5.0, 6.0, 7.0));
        assert_eq!(out[0].from, out[0].to);
        assert_eq!(out[1].to, Vec3::new(5.0, 6.0, 3.0));
    }

    #[test]
    fn departed_collaborator_draws_nothing() {
        let (dom, id) = scene();
        let db = ReflectionDatabase::shared();
        let out = segments(
            &dom,
            db,
            &[who(Status::NotInStudio, vec![id], Some(CAMERA))],
        );
        assert!(out.is_empty());
    }
}
