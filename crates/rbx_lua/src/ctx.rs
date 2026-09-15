//! State shared between every binding.

use std::cell::{RefCell, RefMut};
use std::rc::Rc;

use rbx_dom::WeakDom;
use rbx_reflection::ReflectionDatabase;

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
}

impl Ctx {
    pub(crate) fn new(
        dom: Rc<RefCell<WeakDom>>,
        database: Rc<ReflectionDatabase>,
        output: Rc<RefCell<Vec<String>>>,
    ) -> Self {
        Ctx {
            dom,
            database,
            output,
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

    pub(crate) fn print(&self, line: String) {
        self.output.borrow_mut().push(line);
    }
}
