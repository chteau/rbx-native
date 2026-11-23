use rbx_reflection::ReflectionDatabase;
use serde_json::{json, Value};

use super::super::super::call;
use super::super::super::tests::{place, studio};
use crate::mcp::Session;
use crate::properties::Properties;

/// The error `import_model` answers with before anything is downloaded, or
/// `None` when it went on to the upload.
fn refusal(arguments: Value) -> Option<String> {
    let dom = place();
    let properties = Properties::new(ReflectionDatabase::embedded());
    let studio = studio();
    let mut session = Session::new(&dom, &properties, &studio);
    let result = call(
        &json!({ "name": "import_model", "arguments": arguments }),
        &mut session,
    )
    .unwrap();
    match session.inserting.take() {
        Some(_) => None,
        None => Some(result["content"][0]["text"].as_str().unwrap().to_owned()),
    }
}

#[test]
fn import_model_checks_its_arguments_before_any_upload() {
    let missing = refusal(json!({})).unwrap();
    assert!(
        missing.contains("`path`, or `provider` and `id`"),
        "{missing}"
    );
    assert!(refusal(json!({ "path": "/tmp/notes.txt" }))
        .unwrap()
        .contains("not a .glb"));
    assert!(refusal(json!({ "provider": "sketchfab", "id": "x" }))
        .unwrap()
        .contains("unknown provider"));
    assert!(refusal(json!({ "path": "/tmp/a.glb", "scale": -2 }))
        .unwrap()
        .contains("`scale`"));
    assert!(refusal(json!({ "path": "/tmp/a.glb", "position": [1, 2] }))
        .unwrap()
        .contains("`position`"));
}

#[test]
fn import_model_with_good_arguments_leaves_the_upload_to_the_shell() {
    assert_eq!(
        refusal(json!({ "path": "/tmp/a.glb", "scale": 4, "position": [0, 5, 0] })),
        None
    );
    assert_eq!(
        refusal(json!({ "provider": "polyhaven", "id": "ArmChair_01" })),
        None
    );
}
