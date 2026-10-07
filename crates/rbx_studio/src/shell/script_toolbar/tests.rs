use rbx_dom::WeakDom;

use super::full_name;

#[test]
fn a_script_is_named_from_its_service_down() {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let lava = dom.new_instance("Part", "Lava", Some(workspace));
    let script = dom.new_instance("Script", "Script", Some(lava));
    assert_eq!(full_name(&dom, script), "Workspace.Lava.Script");
    assert_eq!(full_name(&dom, workspace), "Workspace");
}
