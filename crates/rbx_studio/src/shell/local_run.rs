//! Model › Run (local): the place's server scripts and physics run headless
//! on `rbx_sim` for a few seconds of game time, off a copy of the place,
//! and what they printed lands in the Output. Behind its beta switch until
//! Local Run has a viewport of its own.

use gpui_kit::*;

use super::Shell;
use crate::command_bar::Feedback;

pub(crate) const BETA_ID: &str = "local-run";
const SOURCE: &str = "Local Run";
/// Game time one run covers.
const SECONDS: f64 = 5.0;

impl Shell {
    pub(crate) fn run_local(&mut self, cx: &mut Context<Self>) {
        if !crate::beta::enabled(BETA_ID) {
            return;
        }
        let dom = self.dom.clone();
        cx.spawn(async move |this, cx| {
            let rows = cx.background_spawn(async move { simulate(dom) }).await;
            let _ = this.update(cx, |shell, cx| {
                for row in rows {
                    shell.output.push(SOURCE, row);
                }
                cx.notify();
            });
        })
        .detach();
    }
}

fn simulate(dom: rbx_dom::WeakDom) -> Vec<Feedback> {
    let mut sim = match rbx_sim::Simulation::new(dom) {
        Ok(sim) => sim,
        Err(why) => {
            return vec![Feedback::Error(format!(
                "Local Run couldn\u{2019}t start: {why}"
            ))]
        }
    };
    sim.start();
    sim.run_for(SECONDS);
    let output = sim.take_output();
    let mut rows: Vec<Feedback> = output
        .lines()
        .iter()
        .zip(output.kinds())
        .map(|(line, kind)| match kind {
            rbx_lua::LineKind::Print => Feedback::Output(line.clone()),
            rbx_lua::LineKind::Warn => Feedback::Warning(line.clone()),
            rbx_lua::LineKind::Error => Feedback::Error(line.clone()),
        })
        .collect();
    rows.push(Feedback::Info(format!(
        "Local Run: {SECONDS} s of game time, {} frames, nothing written back to the place",
        sim.frame()
    )));
    rows
}
