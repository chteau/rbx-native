use rbx_dom::{Change, Variant, WeakDom};
use rbx_lua::Runtime;
use rbx_reflection::ReflectionDatabase;

use super::resume;

// The Command Bar script that framed a part from a plugin's point of view:
// select it, then put the camera on it. Both have to come out of the step.
#[test]
fn a_run_that_selects_and_moves_the_camera_keeps_the_camera_move() {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let camera = dom.new_instance("Camera", "Camera", Some(workspace));
    dom.set_property(workspace, "CurrentCamera", Variant::Ref(camera))
        .unwrap();
    let part = dom.new_instance("Part", "Part", Some(workspace));
    dom.new_instance("Selection", "Selection", None);
    dom.take_changes();

    let mut runtime = Runtime::lent(&mut dom, ReflectionDatabase::embedded()).unwrap();
    runtime.start(
        "game.Selection:Set({workspace.Part})\n\
         workspace.CurrentCamera.CFrame = CFrame.lookAt(Vector3.new(60, 40, 60), Vector3.zero)\n\
         workspace.CurrentCamera.Focus = CFrame.new()",
    );
    let (_, changes, selection) = resume(&mut runtime, &mut dom, Vec::new());

    assert_eq!(selection, Some(vec![part]));
    let wrote = |name: &str| {
        changes.iter().any(|change| {
            matches!(change, Change::Property { referent, name: written }
                if *referent == camera && written == name)
        })
    };
    assert!(wrote("CFrame") && wrote("Focus"));
    let view = crate::camera::moved_by(&dom, &changes).expect("the run moved the camera");
    assert_eq!(view.eye, [60.0, 40.0, 60.0]);
    assert!(dom.take_changes().is_empty());
}
