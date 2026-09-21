//! A script reading and writing a part a hand-written file left bare: an
//! unstored property reads Roblox's default, and a write lands under the name
//! Roblox saves it as, which is the one the renderer and the save path read.

use rbx_dom::{Instance, Ref, Variant, Vector3Data, WeakDom};
use rbx_lua::Runtime;
use rbx_reflection::ReflectionDatabase;

const WORKSPACE: Ref = Ref::new(1);
const BARE: Ref = Ref::new(2);

fn runtime() -> Runtime {
    let mut dom = WeakDom::new();
    dom.insert(Instance::new(WORKSPACE, "Workspace", "Workspace"));
    dom.insert(Instance::new(BARE, "Part", "Bare"));
    dom.set_parent(BARE, Some(WORKSPACE));
    Runtime::new(dom, ReflectionDatabase::embedded()).expect("runtime must start")
}

#[test]
fn an_unstored_property_reads_its_default() {
    let mut runtime = runtime();
    let output = runtime
        .run(r#"local p = workspace.Bare print(p.Transparency, p.CanCollide, p.BrickColor.Name)"#)
        .expect("script must run");

    assert_eq!(output.lines(), ["0 true Medium stone grey"]);
}

#[test]
fn a_write_lands_under_the_saved_name() {
    let mut runtime = runtime();
    runtime
        .run(
            r#"
            local p = workspace.Bare
            p.Size = Vector3.new(1, 2, 3)
            p.BrickColor = BrickColor.new("Bright red")
        "#,
        )
        .expect("script must run");

    let dom = runtime.dom();
    let part = dom.get(BARE).unwrap().properties();
    assert_eq!(
        part.get("size"),
        Some(&Variant::Vector3(Vector3Data {
            x: 1.0,
            y: 2.0,
            z: 3.0
        }))
    );
    assert_eq!(
        part.get("Color3uint8"),
        Some(&Variant::Color3uint8 {
            r: 196,
            g: 40,
            b: 28
        })
    );
    assert_eq!(part.get("Size"), None);
    assert_eq!(part.get("Color"), None);
}
