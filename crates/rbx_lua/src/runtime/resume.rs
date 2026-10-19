//! Starting a chunk, resuming its threads from the host, cancelling them.

use std::time::{Duration, Instant};

use mlua::{MultiValue, Value, VmState};
use rbx_dom::WeakDom;

use super::{RunState, Runtime, Step};
use crate::engine::changes::{fire_changes, first_number};
use crate::engine::scheduler::{self, Scheduler};
use crate::engine::{engine, signal, Host};
use crate::output::LineKind;

/// How many times one resume fires the changes that event handlers made
/// in turn, before leaving the rest unheard.
const ROUNDS: usize = 8;

const TIMEOUT: &str = "Script timeout: exhausted allowed execution time";

impl Runtime {
    /// Queues `source` as a new run: its chunk starts, in a thread of its
    /// own, at the next [`Runtime::resume`]. Threads earlier runs left
    /// waiting keep going, as in Studio's command bar. A chunk that does not
    /// compile fails that resume. Each run starts with no module loaded.
    pub fn start(&mut self, source: &str) {
        engine(&self.lua).modules.borrow_mut().clear();
        let thread = self
            .lua
            .load(source)
            .set_name("=Command bar")
            .into_function()
            .and_then(|chunk| {
                // The chunk runs under a wrapper, so a chunk that returns
                // `false, x` is not taken for a failed callback (see
                // `scheduler::resume`).
                let main = self.lua.load("(...)()").set_name("=Command bar");
                let thread = self.lua.create_thread(main.into_function()?)?;
                Ok((thread, chunk))
            });
        match thread {
            Ok((thread, chunk)) => {
                self.host.mains.borrow_mut().push(thread.clone());
                let args = MultiValue::from_vec(vec![Value::Function(chunk)]);
                engine(&self.lua)
                    .scheduler
                    .borrow_mut()
                    .deferred
                    .push_back((thread, args));
            }
            Err(error) => self.host.fail(error),
        }
    }

    /// Runs what is due on the runtime's clock — a run's chunk, threads
    /// whose wait is over, deferred threads — then fires the instance events
    /// the DOM changes they made call for, and says what comes next.
    ///
    /// The run gets a fresh time budget (10 seconds) each resume: time spent
    /// sleeping between resumes does not count. The DOM's change log is
    /// read, never drained, so the host still sees every change a run made.
    pub fn resume(&mut self) -> Step {
        let deadline = Instant::now() + self.budget;
        self.lua.set_interrupt(move |_| {
            if Instant::now() >= deadline {
                Err(mlua::Error::runtime(TIMEOUT))
            } else {
                Ok(VmState::Continue)
            }
        });
        let mut seen = self.dom.borrow().changes().len();
        let mut ran = scheduler::tick(&self.lua, self.host.now());
        for _ in 0..ROUNDS {
            if ran.is_err() {
                break;
            }
            let (changes, first) = {
                let dom = self.dom.borrow();
                let fresh = dom.changes()[seen..].to_vec();
                seen += fresh.len();
                let first = first_number(&dom, fresh.len());
                (fresh, first)
            };
            if changes.is_empty() {
                break;
            }
            ran = fire_changes(&self.lua, &changes, Some(first));
        }
        engine(&self.lua).signals.borrow_mut().settle();
        self.lua.remove_interrupt();
        if let Err(error) = ran {
            self.host.output(LineKind::Error, error.to_string());
        }
        let output = std::mem::take(&mut *self.output.borrow_mut());
        Step {
            output,
            state: self.state(),
        }
    }

    /// [`Runtime::resume`] on the host's `dom`, which is the runtime's for
    /// the length of the call only.
    pub fn resume_in(&mut self, dom: &mut WeakDom) -> Step {
        std::mem::swap(dom, &mut self.dom.borrow_mut());
        let step = self.resume();
        std::mem::swap(dom, &mut self.dom.borrow_mut());
        step
    }

    /// Drops every waiting thread, timer and deferred call, and every
    /// connection the runs made: nothing they left behind runs again.
    pub fn cancel(&mut self) {
        let engine = engine(&self.lua);
        *engine.scheduler.borrow_mut() = Scheduler::default();
        signal::disconnect_all(&self.lua);
        self.host.mains.borrow_mut().clear();
    }

    fn state(&mut self) -> RunState {
        let failure = self.host.failure.borrow_mut().take();
        if let Some(error) = failure {
            self.cancel();
            return RunState::Failed(error);
        }
        let now = self.host.now();
        let due = engine(&self.lua).scheduler.borrow().next_due(now);
        match due {
            Some(due) => RunState::Pending {
                wake_in: Duration::from_secs_f64((due - now).max(0.0)),
            },
            None => {
                self.host.mains.borrow_mut().clear();
                RunState::Finished
            }
        }
    }
}
