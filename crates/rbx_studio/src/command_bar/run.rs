//! Runs one chunk against the place's DOM, the way `rbx_lua::Runtime` needs
//! to: it owns the tree while a script runs, so the caller's slot has to give
//! it up and get it back, whatever the script did.

use rbx_dom::WeakDom;
use rbx_lua::Runtime;
use rbx_reflection::ReflectionDatabase;

/// Takes `dom` by value and always hands one back: the mutated tree on
/// success, whatever the script managed to change before an error (Studio's
/// own command bar keeps a partial mutation too), or — only if the VM itself
/// could not be built, an engine fault rather than a script one — an empty
/// placeholder, since nothing else survives that path.
pub(crate) fn run(
    dom: WeakDom,
    database: &ReflectionDatabase,
    source: &str,
) -> (WeakDom, Result<Vec<String>, String>) {
    let mut runtime = match Runtime::new(dom, database.clone()) {
        Ok(runtime) => runtime,
        Err(err) => return (WeakDom::new(), Err(err.to_string())),
    };

    let result = runtime
        .run(source)
        .map(|output| output.lines().to_vec())
        .map_err(|err| err.to_string());
    (runtime.into_dom(), result)
}

#[cfg(test)]
mod tests {
    use rbx_dom::{Instance, Ref};

    use super::*;

    fn synthetic_dom() -> WeakDom {
        let mut dom = WeakDom::new();
        dom.insert(Instance::new(Ref::new(1), "Workspace", "Workspace"));
        dom
    }

    #[test]
    fn a_successful_script_hands_back_the_mutated_dom() {
        let dom = synthetic_dom();
        let database = ReflectionDatabase::embedded();

        let (dom, result) = run(
            dom,
            &database,
            r#"Instance.new("Folder", workspace).Name = "FromScript""#,
        );

        result.expect("script should run");
        let workspace = dom.get(Ref::new(1)).expect("workspace should survive");
        let child = dom
            .get(workspace.children()[0])
            .expect("the folder should have been created");
        assert_eq!(child.name(), "FromScript");
    }

    #[test]
    fn print_output_is_captured_in_order() {
        let dom = synthetic_dom();
        let database = ReflectionDatabase::embedded();

        let (_, result) = run(dom, &database, "print(1) print(2)");

        assert_eq!(result.expect("script should run"), vec!["1", "2"]);
    }

    #[test]
    fn a_runtime_error_still_returns_the_dom_the_script_had_already_mutated() {
        let dom = synthetic_dom();
        let database = ReflectionDatabase::embedded();

        let (dom, result) = run(
            dom,
            &database,
            r#"Instance.new("Folder", workspace).Name = "Kept" error("boom")"#,
        );

        assert!(result.is_err());
        let workspace = dom.get(Ref::new(1)).expect("workspace should survive");
        let child = dom
            .get(workspace.children()[0])
            .expect("the mutation before the error should have stuck");
        assert_eq!(child.name(), "Kept");
    }

    #[test]
    fn a_syntax_error_leaves_the_dom_untouched() {
        let dom = synthetic_dom();
        let database = ReflectionDatabase::embedded();

        let (dom, result) = run(dom, &database, "this is not luau");

        assert!(result.is_err());
        let workspace = dom.get(Ref::new(1)).expect("workspace should survive");
        assert!(workspace.children().is_empty());
    }
}
