//! `game:GetService("Selection")` for the command bar. The editor hands the
//! current selection in before a run and takes back whatever the run set.

use mlua::{Lua, Result, Table, Value};
use rbx_dom::Ref;

use crate::ctx::Ctx;
use crate::engine::signal::{self, LuaSignal, Owner, SignalKey};
use crate::instance::LuaInstance;

/// The selection as the running script sees it, kept as app data.
#[derive(Default)]
pub(super) struct Selection {
    refs: Vec<Ref>,
    written: bool,
}

fn changed() -> SignalKey {
    SignalKey::new(Owner::Service("Selection"), "SelectionChanged")
}

/// Loads the editor's selection, unless a run has set one the editor has
/// not collected yet.
pub(super) fn load(lua: &Lua, refs: Vec<Ref>) {
    if let Some(mut slot) = lua.app_data_mut::<Selection>() {
        if !slot.written {
            slot.refs = refs;
        }
    }
}

/// What a run set since the last call, if it set anything.
pub(super) fn take(lua: &Lua) -> Option<Vec<Ref>> {
    let mut slot = lua.app_data_mut::<Selection>()?;
    std::mem::take(&mut slot.written).then(|| slot.refs.clone())
}

fn current(lua: &Lua, ctx: &Ctx) -> Vec<Ref> {
    let dom = ctx.dom();
    lua.app_data_ref::<Selection>()
        .map(|slot| {
            slot.refs
                .iter()
                .copied()
                .filter(|r| dom.get(*r).is_some())
                .collect()
        })
        .unwrap_or_default()
}

fn write(lua: &Lua, refs: Vec<Ref>) -> Result<()> {
    if let Some(mut slot) = lua.app_data_mut::<Selection>() {
        slot.refs = refs;
        slot.written = true;
    }
    signal::fire(lua, &changed(), &[])
}

/// Studio's own wording for both mistakes, read off its command bar.
fn refs_of(list: Value) -> Result<Vec<Ref>> {
    let Value::Table(list): Value = list else {
        return Err(mlua::Error::runtime(format!(
            "Expected table argument, got {}",
            match list {
                Value::Integer(_) | Value::Number(_) => "number",
                _ => list.type_name(),
            }
        )));
    };
    sequence(&list)
}

fn sequence(list: &Table) -> Result<Vec<Ref>> {
    list.clone()
        .sequence_values::<Value>()
        .map(|value| {
            value?
                .as_userdata()
                .and_then(|data| data.borrow::<LuaInstance>().ok().map(|i| i.referent()))
                .ok_or_else(|| mlua::Error::runtime("Unable to cast value to Object"))
        })
        .collect()
}

fn unique(refs: Vec<Ref>) -> Vec<Ref> {
    let mut out = Vec::with_capacity(refs.len());
    for r in refs {
        if !out.contains(&r) {
            out.push(r);
        }
    }
    out
}

pub(super) fn member(lua: &Lua, ctx: &Ctx, key: &str) -> Result<Option<Value>> {
    let ctx = ctx.clone();
    let function = match key {
        "Get" => lua.create_function(move |lua, _: Value| {
            let table = lua.create_table()?;
            for (i, r) in current(lua, &ctx).into_iter().enumerate() {
                table.raw_set(i + 1, LuaInstance::new(r, ctx.clone()))?;
            }
            Ok(table)
        })?,
        "Set" => lua.create_function(move |lua, (_, list): (Value, Value)| {
            write(lua, unique(refs_of(list)?))
        })?,
        "Add" => lua.create_function(move |lua, (_, list): (Value, Value)| {
            let mut refs = current(lua, &ctx);
            refs.extend(refs_of(list)?);
            write(lua, unique(refs))
        })?,
        "Remove" => lua.create_function(move |lua, (_, list): (Value, Value)| {
            let gone = refs_of(list)?;
            let mut refs = current(lua, &ctx);
            refs.retain(|r| !gone.contains(r));
            write(lua, refs)
        })?,
        "SelectionChanged" => {
            return Ok(Some(Value::UserData(
                lua.create_userdata(LuaSignal(changed()))?,
            )))
        }
        _ => return Ok(None),
    };
    Ok(Some(Value::Function(function)))
}

#[cfg(test)]
mod tests {
    use rbx_dom::{Ref, WeakDom};
    use rbx_reflection::ReflectionDatabase;

    use super::super::Runtime;

    fn with_workspace() -> (Runtime, Ref) {
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        (
            Runtime::new(dom, ReflectionDatabase::embedded()).unwrap(),
            workspace,
        )
    }

    fn lines(runtime: &mut Runtime, source: &str) -> Vec<String> {
        runtime.run(source).unwrap().lines().to_vec()
    }

    const S: &str = "local s = game:GetService('Selection') ";

    #[test]
    fn get_answers_with_what_the_editor_has_selected() {
        let (mut runtime, workspace) = with_workspace();
        runtime.set_selection(vec![workspace]);
        let source = format!("{S}print(#s:Get(), s:Get()[1], s.ClassName, typeof(s))");
        assert_eq!(lines(&mut runtime, &source), ["1 Workspace Selection Instance"]);
        assert_eq!(runtime.take_selection(), None);
    }

    #[test]
    fn set_add_and_remove_reach_the_editor_once() {
        let (mut runtime, workspace) = with_workspace();
        let source = format!("{S}s:Set({{workspace}}) s:Add({{workspace}}) print(#s:Get())");
        assert_eq!(lines(&mut runtime, &source), ["1"]);
        assert_eq!(runtime.take_selection(), Some(vec![workspace]));
        assert_eq!(runtime.take_selection(), None);

        lines(&mut runtime, &format!("{S}s:Remove({{workspace}})"));
        assert_eq!(runtime.take_selection(), Some(vec![]));
    }

    #[test]
    fn a_selection_the_editor_has_not_collected_is_not_overwritten() {
        let (mut runtime, workspace) = with_workspace();
        lines(&mut runtime, &format!("{S}s:Set({{workspace}})"));
        runtime.set_selection(vec![]);
        assert_eq!(runtime.take_selection(), Some(vec![workspace]));
    }

    #[test]
    fn bad_arguments_fail_with_studios_words() {
        let (mut runtime, _) = with_workspace();
        let source = format!(
            "{S}print(pcall(function() s:Set(5) end)) print(pcall(function() s:Set({{5}}) end))"
        );
        let printed = lines(&mut runtime, &source);
        assert!(printed[0].ends_with("Expected table argument, got number"), "{printed:?}");
        assert!(printed[1].ends_with("Unable to cast value to Object"), "{printed:?}");
        assert_eq!(runtime.take_selection(), None);
    }

    #[test]
    fn selection_changed_fires_on_a_set() {
        let (mut runtime, _) = with_workspace();
        let source = format!(
            "{S}s.SelectionChanged:Connect(function() print('fired') end) s:Set({{workspace}})"
        );
        assert_eq!(lines(&mut runtime, &source), ["fired"]);
    }
}
