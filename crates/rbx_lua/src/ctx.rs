//! State shared between every binding.

use std::cell::{RefCell, RefMut};
use std::rc::Rc;

use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::output::{LineKind, Output};
use crate::printed::PrintPart;

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
    output: Rc<RefCell<Output>>,
    guard: Option<WriteGuard>,
    /// Told about every write before it is checked; see `WriteHook`.
    on_write: Option<WriteHook>,
    /// Class and name of instances scripts destroyed: Roblox keeps a
    /// destroyed instance readable (`Parent` nil), so a handler that gets
    /// one late can still ask what it was.
    tombstones: Rc<RefCell<std::collections::HashMap<Ref, (String, String)>>>,
}

/// Decides whether a script may write to an instance; `Err` carries the reason.
pub(crate) type WriteGuard = Rc<dyn Fn(&WeakDom, Ref) -> Result<(), String>>;

/// Sees the DOM as it stands just before a write to an instance (the
/// plugin host's undo point copies the place here, see `plugin::undo`).
pub(crate) type WriteHook = Rc<dyn Fn(&WeakDom, Ref)>;

impl Ctx {
    pub(crate) fn with_guard(mut self, guard: WriteGuard) -> Self {
        self.guard = Some(guard);
        self
    }

    pub(crate) fn with_write_hook(mut self, hook: WriteHook) -> Self {
        self.on_write = Some(hook);
        self
    }

    /// Borrows the DOM immutably, so call it before `dom_mut()`.
    pub(crate) fn check_write(&self, referent: Ref) -> mlua::Result<()> {
        if let Some(hook) = &self.on_write {
            hook(&self.dom.borrow(), referent);
        }
        match &self.guard {
            Some(guard) => guard(&self.dom.borrow(), referent).map_err(mlua::Error::runtime),
            None => Ok(()),
        }
    }

    pub(crate) fn new(
        dom: Rc<RefCell<WeakDom>>,
        database: Rc<ReflectionDatabase>,
        output: Rc<RefCell<Output>>,
    ) -> Self {
        Ctx {
            dom,
            database,
            output,
            guard: None,
            on_write: None,
            tombstones: Rc::default(),
        }
    }

    /// Notes `root` and its descendants as destroyed; call before removing.
    pub(crate) fn bury(&self, dom: &WeakDom, root: Ref) {
        let mut tombstones = self.tombstones.borrow_mut();
        let mut stack = vec![root];
        while let Some(r) = stack.pop() {
            if let Some(i) = dom.get(r) {
                tombstones.insert(r, (i.class().to_string(), i.name().to_string()));
                stack.extend_from_slice(i.children());
            }
        }
    }

    /// The class and name a destroyed instance had.
    pub(crate) fn tombstone(&self, referent: Ref) -> Option<(String, String)> {
        self.tombstones.borrow().get(&referent).cloned()
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

    pub(crate) fn print(&self, kind: LineKind, parts: Vec<PrintPart>) {
        self.output.borrow_mut().push(kind, parts);
    }
}
