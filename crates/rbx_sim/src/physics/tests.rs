use rbx_dom::{CFrameData, Vector3Data};

use super::{cframe_of, pose_of};

#[test]
fn cframe_round_trips_through_a_pose() {
    let c = std::f32::consts::FRAC_1_SQRT_2;
    // 45 degrees about Y, row-major.
    let cframe = CFrameData {
        position: Vector3Data {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        },
        rotation: [c, 0.0, c, 0.0, 1.0, 0.0, -c, 0.0, c],
    };
    let back = cframe_of(&pose_of(&cframe));
    assert_eq!(back.position, cframe.position);
    for (a, b) in back.rotation.iter().zip(cframe.rotation) {
        assert!((a - b).abs() < 1e-6, "{:?} vs {:?}", back.rotation, cframe.rotation);
    }
}
