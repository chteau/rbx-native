//! State shared between every binding.

use std::cell::{RefCell, RefMut};
use std::rc::Rc;

use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::printed::{self, PrintPart};

/// Handle to the DOM, the reflection data and the captured output.
///
/// Every userdata needs the same three things, and mlua requires userdata to own
/// its state (`'static`), so they are shared through `Rc`. `RefCell` rather than
/// a lock because a `Runtime` is single-threaded by construction: mlua is built
/// without its `send` feature, so nothing here can cross a thread boundary.
#[derive(Clone)]
pub(crate) struct Ctx {
    dom: Rc<RefCell<WeakDom>>,
    database: Rc<ReflectionDatabase>,
    output: Rc<RefCell<Vec<String>>>,
    /// Each `print` call's arguments, structured — see `printed`. Parallel to
    /// `output` for a plain run; the debugger drains `output` alone.
    printed: Rc<RefCell<Vec<Vec<PrintPart>>>>,
    guard: Option<WriteGuard>,
}

/// Decides whether a script may write to an instance; `Err` carries the reason.
pub(crate) type WriteGuard = Rc<dyn Fn(&WeakDom, Ref) -> Result<(), String>>;

impl Ctx {
    pub(crate) fn with_guard(mut self, guard: WriteGuard) -> Self {
        self.guard = Some(guard);
        self
    }

    /// Borrows the DOM immutably, so call it before `dom_mut()`.
    pub(crate) fn check_write(&self, referent: Ref) -> mlua::Result<()> {
        match &self.guard {
            Some(guard) => guard(&self.dom.borrow(), referent).map_err(mlua::Error::runtime),
            None => Ok(()),
        }
    }

    pub(crate) fn new(
        dom: Rc<RefCell<WeakDom>>,
        database: Rc<ReflectionDatabase>,
        output: Rc<RefCell<Vec<String>>>,
        printed: Rc<RefCell<Vec<Vec<PrintPart>>>>,
    ) -> Self {
        Ctx {
            dom,
            database,
            output,
            printed,
            guard: None,
        }
    }

    pub(crate) fn dom(&self) -> std::cell::Ref<'_, WeakDom> {
        self.dom.borrow()
    }

    pub(crate) fn dom_mut(&self) -> RefMut<'_, WeakDom> {
        self.dom.borrow_mut()
    }

    pub(crate) fn database(&self) -> &ReflectionDatabase {
        &self.database
    }

    pub(crate) fn print(&self, parts: Vec<PrintPart>) {
        self.output.borrow_mut().push(printed::text(&parts));
        self.printed.borrow_mut().push(parts);
    }
}
