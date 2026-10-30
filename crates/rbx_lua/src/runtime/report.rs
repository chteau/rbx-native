//! Runs a chunk and reports its first return value, for tools that answer
//! with what the code produced.

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{Function, MultiValue, Value};

use super::Runtime;
use crate::engine::require;
use crate::output::LineKind;

#[cfg(test)]
mod tests;

/// Studio's name for code run over MCP, so errors read `AssistantCommand:1: ...`.
const CHUNK: &str = "=AssistantCommand";

/// A reported run's answer once it is known: Ok(serialized value) or Err(error message).
pub type Reported = Rc<RefCell<Option<Result<String, String>>>>;

/// Runs the chunk under `xpcall` so an error raised after a yield is caught
/// too, and hands the outcome to `done`.
const WRAPPER: &str = "local f, serialize, done = ...; \
    done(xpcall(function() return serialize((f())) end, function(e) return tostring(e) end))";

impl Runtime {
    /// Queues `source` like `start`, REPL-style: `"return " + source` is compiled first so a bare
    /// expression yields its value, falling back to `source` itself. `serializer` is Luau source of a
    /// chunk that returns `function(value): string`. The chunk's first return value goes through it
    /// into the slot as Ok. A runtime error, also one raised after a yield in a later resume, lands as
    /// Err(message). When neither form compiles the slot is Err(compile error) at once and nothing
    /// is queued.
    pub fn start_reported(&mut self, source: &str, serializer: &str) -> Reported {
        let slot: Reported = Rc::new(RefCell::new(None));
        // Studio runs this code with no requiring context at all, unlike the
        // command bar, and at identity 10, so it gets its own `require` and
        // `printidentity`.
        let scope = match self.scope() {
            Ok(scope) => scope,
            Err(error) => {
                *slot.borrow_mut() = Some(Err(error.to_string()));
                return slot;
            }
        };
        let load = |text: &str| {
            self.lua
                .load(text)
                .set_name(CHUNK)
                .set_environment(scope.clone())
                .into_function()
        };
        let chunk = match load(&format!("return {source}")) {
            Ok(chunk) => chunk,
            Err(_) => match load(source) {
                Ok(chunk) => chunk,
                Err(error) => {
                    *slot.borrow_mut() = Some(Err(error.to_string()));
                    // Queued anyway so the run fails the way a Command Bar run
                    // does, with its error row in Output.
                    self.start_call(Err(error), MultiValue::new());
                    return slot;
                }
            },
        };
        match self.reporter(chunk, serializer, &slot) {
            Ok((wrapper, args)) => self.start_call(Ok(wrapper), args),
            Err(error) => *slot.borrow_mut() = Some(Err(error.to_string())),
        }
        slot
    }

    /// The chunk's environment: its own `require` and `printidentity`, the
    /// globals behind them.
    fn scope(&self) -> mlua::Result<mlua::Table> {
        let scope = self.lua.create_table()?;
        scope.set("require", require::require_for(&self.lua, Value::Nil)?)?;
        let ctx = crate::engine::engine(&self.lua).ctx.clone();
        scope.set(
            "printidentity",
            super::globals::printidentity(&self.lua, &ctx, 10)?,
        )?;
        let meta = self.lua.create_table()?;
        meta.set("__index", self.lua.globals())?;
        scope.set_metatable(Some(meta))?;
        Ok(scope)
    }

    fn reporter(
        &self,
        chunk: Function,
        serializer: &str,
        slot: &Reported,
    ) -> mlua::Result<(Function, MultiValue)> {
        let serialize: Function = self.lua.load(serializer).call(())?;
        let target = slot.clone();
        let output = self.output.clone();
        let done = self
            .lua
            .create_function(move |_, (ok, value): (bool, Value)| {
                let text = match &value {
                    Value::String(text) => text.to_string_lossy().to_string(),
                    other => format!("{other:?}"),
                };
                if !ok {
                    // Caught here, so Output would otherwise show a success.
                    output.borrow_mut().push_text(LineKind::Error, text.clone());
                }
                *target.borrow_mut() = Some(if ok { Ok(text) } else { Err(text) });
                Ok(())
            })?;
        let wrapper = self.lua.load(WRAPPER).set_name("=run").into_function()?;
        let args = MultiValue::from_iter([
            Value::Function(chunk),
            Value::Function(serialize),
            Value::Function(done),
        ]);
        Ok((wrapper, args))
    }
}
