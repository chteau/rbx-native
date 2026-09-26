//! One debug run of a script, on a thread of its own.
//!
//! A paused script is a Luau VM blocked inside its step hook (see
//! `rbx_lua`'s debugger), so it cannot share the UI thread: the run gets its
//! own OS thread and a clone of the place, and talks to the editor over two
//! channels — [`Event`]s out, [`Command`]s in — the same `std::sync::mpsc`
//! shape `argon_client::thread` uses. The editor decides what to do with
//! the mutated clone once the run is over.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;

use rbx_dom::WeakDom;
use rbx_lua::{Breakpoint, Frame, Paused, Resume, Runtime, Variable};
use rbx_reflection::ReflectionDatabase;

/// What the editor asks of a paused run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Command {
    Resume(Resume),
    /// Evaluates each watch expression, answered with one
    /// [`Event::Evaluated`] naming each expression beside its value, so an
    /// answer that crosses an edit of the watch list still lands right.
    Evaluate(Vec<String>),
}

/// Everything the Watch and Call Stack docks show for one pause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Pause {
    pub(crate) line: u32,
    pub(crate) stack: Vec<Frame>,
    pub(crate) variables: Vec<Variable>,
    /// Printed since the last pause, so the Output dock is current while the
    /// script is stopped.
    pub(crate) output: Vec<String>,
}

pub(crate) type Evaluation = Result<String, String>;

#[derive(Debug)]
pub(crate) enum Event {
    Paused(Pause),
    Evaluated(Vec<(String, Evaluation)>),
    Finished(Finished),
}

#[derive(Debug)]
pub(crate) struct Finished {
    /// The clone as the script left it — `None` only if the VM itself could
    /// not be built, which leaves nothing worth keeping.
    pub(crate) dom: Option<WeakDom>,
    /// Printed since the last pause.
    pub(crate) output: Vec<String>,
    pub(crate) result: Result<(), String>,
}

/// The editor's end of a run. Dropping it stops a paused script: the run
/// sees its command channel close and stops the way the Stop button does.
pub(crate) struct Session {
    commands: Sender<Command>,
    events: Receiver<Event>,
    stop: Arc<AtomicBool>,
}

impl Session {
    pub(crate) fn start(
        dom: WeakDom,
        database: ReflectionDatabase,
        source: String,
        name: String,
        breakpoints: Vec<Breakpoint>,
    ) -> Session {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = stop.clone();
        thread::Builder::new()
            .name("script-debugger".into())
            .spawn(move || {
                let finished = run(dom, database, &source, &name, &breakpoints, stop_flag, {
                    let events = event_tx.clone();
                    move |paused| on_pause(paused, &events, &command_rx)
                });
                let _ = event_tx.send(Event::Finished(finished));
            })
            .expect("spawning the script debugger thread");
        Session {
            commands: command_tx,
            events: event_rx,
            stop,
        }
    }

    /// Stops the script whether it is paused or still running.
    pub(crate) fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        self.send(Command::Resume(Resume::Stop));
    }

    /// A run that already finished has nobody listening; that is not an
    /// error worth surfacing.
    pub(crate) fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    /// Whatever has arrived since the last call, without blocking.
    pub(crate) fn events(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }

    #[cfg(test)]
    fn next_event(&self) -> Event {
        self.events
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the debugger thread should report back")
    }
}

fn run(
    dom: WeakDom,
    database: ReflectionDatabase,
    source: &str,
    name: &str,
    breakpoints: &[Breakpoint],
    stop: Arc<AtomicBool>,
    on_pause: impl FnMut(&Paused) -> Resume + 'static,
) -> Finished {
    let mut runtime = match Runtime::new(dom, database) {
        Ok(runtime) => runtime,
        Err(err) => {
            return Finished {
                dom: None,
                output: Vec::new(),
                result: Err(err.to_string()),
            }
        }
    };
    let (output, result) = match runtime.debug(source, name, breakpoints, stop, on_pause) {
        Ok(output) => (output.lines().to_vec(), Ok(())),
        Err(err) => (Vec::new(), Err(err.to_string())),
    };
    Finished {
        dom: Some(runtime.into_dom()),
        output,
        result,
    }
}

/// Reports the pause, then answers commands until one resumes the script.
fn on_pause(paused: &Paused, events: &Sender<Event>, commands: &Receiver<Command>) -> Resume {
    let pause = Pause {
        line: paused.line(),
        stack: paused.call_stack(),
        variables: paused.variables(),
        output: paused.take_output(),
    };
    if events.send(Event::Paused(pause)).is_err() {
        return Resume::Stop;
    }
    loop {
        match commands.recv() {
            Ok(Command::Resume(resume)) => return resume,
            Ok(Command::Evaluate(expressions)) => {
                let values = expressions
                    .iter()
                    .map(|expression| (expression.clone(), paused.evaluate(expression)))
                    .collect();
                if events.send(Event::Evaluated(values)).is_err() {
                    return Resume::Stop;
                }
            }
            Err(_) => return Resume::Stop,
        }
    }
}

#[cfg(test)]
mod tests {
    use rbx_dom::{Instance, Ref};

    use super::*;

    fn start(source: &str, lines: &[u32]) -> Session {
        let mut dom = WeakDom::new();
        dom.insert(Instance::new(Ref::new(1), "Workspace", "Workspace"));
        let breakpoints = lines
            .iter()
            .map(|&line| Breakpoint {
                line,
                ..Breakpoint::default()
            })
            .collect();
        Session::start(
            dom,
            ReflectionDatabase::embedded(),
            source.into(),
            "Script".into(),
            breakpoints,
        )
    }

    fn paused(event: Event) -> Pause {
        match event {
            Event::Paused(pause) => pause,
            other => panic!("expected a pause, got {other:?}"),
        }
    }

    fn finished(event: Event) -> Finished {
        match event {
            Event::Finished(finished) => finished,
            other => panic!("expected the run to finish, got {other:?}"),
        }
    }

    #[test]
    fn a_run_pauses_answers_watches_and_resumes_to_the_end() {
        let session = start(
            "print(\"before\")\nlocal n = 41\nn += 1\nInstance.new(\"Folder\", workspace)\nprint(n)",
            &[3],
        );

        let pause = paused(session.next_event());
        assert_eq!(pause.line, 3);
        assert_eq!(pause.output, vec!["before"]);
        assert_eq!(pause.stack[0].function, "main chunk");
        assert_eq!(pause.variables[0].name, "n");

        session.send(Command::Evaluate(vec!["n * 2".into(), "nope()".into()]));
        let Event::Evaluated(values) = session.next_event() else {
            panic!("expected watch values");
        };
        assert_eq!(values[0], ("n * 2".to_owned(), Ok("82".to_owned())));
        assert!(values[1].1.is_err());

        session.send(Command::Resume(Resume::Continue));
        let done = finished(session.next_event());
        assert_eq!(done.result, Ok(()));
        assert_eq!(done.output, vec!["42"]);
        let dom = done.dom.expect("the clone comes back");
        assert_eq!(dom.get(Ref::new(1)).unwrap().children().len(), 1);
    }

    #[test]
    fn dropping_the_session_stops_a_paused_run() {
        let session = start("local a = 1\nlocal b = 2", &[2]);
        paused(session.next_event());
        let Session {
            commands, events, ..
        } = session;
        drop(commands);
        let done = finished(
            events
                .recv_timeout(std::time::Duration::from_secs(10))
                .unwrap(),
        );
        assert!(done.result.unwrap_err().contains(rbx_lua::STOPPED));
    }

    #[test]
    fn stop_ends_a_script_that_never_pauses() {
        let mut dom = WeakDom::new();
        dom.insert(Instance::new(Ref::new(1), "Workspace", "Workspace"));
        let session = Session::start(
            dom,
            ReflectionDatabase::embedded(),
            "while true do end".into(),
            "Script".into(),
            Vec::new(),
        );
        session.stop();
        let Event::Finished(done) = session.next_event() else {
            panic!("expected the run to finish");
        };
        assert!(done.result.unwrap_err().contains(rbx_lua::STOPPED));
    }
}
