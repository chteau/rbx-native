//! "Import from Figma…": the sidebar panel that signs in to Figma (OAuth 2
//! with PKCE, see `rbx_figma::oauth`), reads a frame link, and inserts the
//! inferred tree (see `rbx_figma::infer`) under the canvas's `ScreenGui` as
//! one undo step, its pictures uploaded through the Open Cloud key first.
//! Guesses the inference wasn't sure of stay listed under "Review" until
//! their instances are gone.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{v_flex, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_cloud::{ApiKey, Client};
use rbx_dom::{Ref, WeakDom};
use rbx_figma::api::Session;
use rbx_figma::import::Cache;
use rbx_figma::infer::Node;
use rbx_figma::link::{self, Link};
use rbx_figma::oauth::{self, Tokens};

use super::super::chrome;
use super::Shell;
use crate::command_bar::Feedback;
use crate::{explorer, figma_store, tokens};

const SOURCE: &str = "Figma import";
/// How long the browser has to come back from Figma's consent page.
const SIGN_IN_WAIT: Duration = Duration::from_secs(180);

/// The panel's own state; the imported tree lives only in the place.
pub(super) struct Figma {
    pub(super) open: bool,
    url: Entity<InputState>,
    tokens: Option<Tokens>,
    busy: bool,
    progress: Arc<Mutex<String>>,
    status: Option<(bool, String)>,
    review: Vec<(Ref, String)>,
}

impl Figma {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Shell>) -> Self {
        Figma {
            open: false,
            url: cx.new(|cx| InputState::new(window, cx).placeholder("Paste a Figma frame link")),
            tokens: None,
            busy: false,
            progress: Arc::default(),
            status: None,
            review: Vec::new(),
        }
    }
}

impl Shell {
    pub(super) fn open_figma(&mut self, cx: &mut Context<Self>) {
        self.ui.figma.open = true;
        if self.ui.figma.tokens.is_none() {
            cx.spawn(async move |this, cx| {
                let stored = figma_store::load(cx).await.ok().flatten();
                let _ = this.update(cx, |shell, cx| {
                    shell.ui.figma.tokens = shell.ui.figma.tokens.take().or(stored);
                    cx.notify();
                });
            })
            .detach();
        }
        cx.notify();
    }

    fn figma_status(&mut self, error: bool, text: String, cx: &mut Context<Self>) {
        let feedback = match error {
            true => Feedback::Error(text.clone()),
            false => Feedback::Output(text.clone()),
        };
        self.output.push(SOURCE, feedback.clone());
        self.command_bar.set_feedback(feedback);
        self.ui.figma.status = Some((error, text));
        self.ui.figma.busy = false;
        cx.notify();
    }

    fn figma_connect(&mut self, cx: &mut Context<Self>) {
        let pending = match oauth::begin() {
            Ok(pending) => pending,
            Err(err) => return self.figma_status(true, err, cx),
        };
        cx.open_url(&pending.url);
        self.figma_busy("Waiting for Figma in your browser\u{2026}", cx);
        cx.spawn(async move |this, cx| {
            let signed_in = cx
                .background_spawn(async move { pending.finish(SIGN_IN_WAIT) })
                .await;
            if let Ok(tokens) = &signed_in {
                if let Err(err) = figma_store::save(tokens, cx).await {
                    eprintln!("Figma sign-in not stored: {err}");
                }
            }
            let _ = this.update(cx, |shell, cx| match signed_in {
                Ok(tokens) => {
                    shell.ui.figma.tokens = Some(tokens);
                    shell.figma_status(false, "Connected to Figma.".into(), cx);
                }
                Err(err) => shell.figma_status(true, err, cx),
            });
        })
        .detach();
    }

    fn figma_disconnect(&mut self, cx: &mut Context<Self>) {
        self.ui.figma.tokens = None;
        cx.spawn(async move |_, cx| {
            if let Err(err) = figma_store::forget(cx).await {
                eprintln!("Figma sign-in not removed: {err}");
            }
        })
        .detach();
        self.figma_status(false, "Disconnected from Figma.".into(), cx);
    }

    /// Marks the panel busy and redraws it while the progress line moves.
    fn figma_busy(&mut self, first: &str, cx: &mut Context<Self>) {
        self.ui.figma.busy = true;
        self.ui.figma.status = None;
        *self.ui.figma.progress.lock().unwrap() = first.to_string();
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(250))
                .await;
            let busy = this.update(cx, |shell, cx| {
                cx.notify();
                shell.ui.figma.busy
            });
            if !matches!(busy, Ok(true)) {
                break;
            }
        })
        .detach();
        cx.notify();
    }

    fn figma_import(&mut self, cx: &mut Context<Self>) {
        let Some(screen) = self.canvas_request().map(|request| request.screen) else {
            return self.figma_status(true, "Open a ScreenGui on the canvas first.".into(), cx);
        };
        let Some(tokens) = self.ui.figma.tokens.clone() else {
            return self.figma_status(true, "Connect to Figma first.".into(), cx);
        };
        let link = match link::parse(&self.ui.figma.url.read(cx).value()) {
            Ok(link) => link,
            Err(err) => return self.figma_status(true, err, cx),
        };
        self.figma_busy("Starting\u{2026}", cx);
        let progress = self.ui.figma.progress.clone();
        cx.spawn(async move |this, cx| {
            let done = cx
                .background_spawn(async move { fetch(tokens, &link, progress) })
                .await;
            if let Some(tokens) = done.as_ref().ok().and_then(|(_, t)| t.clone()) {
                if let Err(err) = figma_store::save(&tokens, cx).await {
                    eprintln!("Refreshed Figma sign-in not stored: {err}");
                }
            }
            let _ = this.update(cx, |shell, cx| match done {
                Ok((tree, refreshed)) => {
                    if let Some(tokens) = refreshed {
                        shell.ui.figma.tokens = Some(tokens);
                    }
                    shell.insert_figma_tree(screen, tree, cx);
                }
                Err(err) => shell.figma_status(true, err, cx),
            });
        })
        .detach();
    }

    /// The whole tree under `screen`, as one undo step.
    fn insert_figma_tree(&mut self, screen: Ref, tree: Node, cx: &mut Context<Self>) {
        if self.dom.get(screen).is_none() {
            return self.figma_status(
                true,
                "The ScreenGui was removed during the import.".into(),
                cx,
            );
        }
        self.push_history();
        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        let mut review = Vec::new();
        let root = materialize(&mut dom, &self.database, screen, tree, &mut review);
        self.dom = dom;
        let changes = self.dom.take_changes();
        self.rebuild_explorer(cx);
        self.select(root, cx);
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        let count = review.len();
        self.ui.figma.review = review;
        let note = match count {
            0 => String::new(),
            n => format!(" {n} guess{} to review.", if n == 1 { "" } else { "es" }),
        };
        self.figma_status(false, format!("Imported from Figma.{note}"), cx);
    }

    /// The panel that stands in for the sidebar while it is open.
    pub(super) fn figma_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        // A review entry whose instance went away has nothing left to show.
        let dom = &self.dom;
        self.ui.figma.review.retain(|(r, _)| dom.get(*r).is_some());
        let figma = &self.ui.figma;
        let connected = figma.tokens.is_some();
        let busy = figma.busy;
        let line = |text: SharedString, color: Rgba| {
            div()
                .px(px(4.))
                .text_size(tokens::text_sm())
                .text_color(color)
                .child(text)
        };
        let action = |id: &'static str, label: &'static str, enabled: bool| {
            chrome::button(id, label, false).when(!enabled, |this| this.opacity(0.5))
        };
        let mut body = v_flex().w_full().gap(px(8.));
        if oauth::CLIENT_SECRET.is_none() {
            body = body.child(line(oauth::NO_SECRET.into(), tokens::text_muted()));
        } else if connected {
            body = body.child(action("figma-disconnect", "Disconnect", !busy).on_click(
                cx.listener(move |shell, _, _, cx| {
                    if !busy {
                        shell.figma_disconnect(cx);
                    }
                }),
            ));
        } else {
            body = body.child(action("figma-connect", "Connect to Figma", !busy).on_click(
                cx.listener(move |shell, _, _, cx| {
                    if !busy {
                        shell.figma_connect(cx);
                    }
                }),
            ));
        }
        let can_import = connected && !busy;
        body = body
            .child(Input::new(&figma.url).with_size(tokens::field_size()))
            .child(
                action("figma-import", "Import", can_import).on_click(cx.listener(
                    move |shell, _, _, cx| {
                        if can_import {
                            shell.figma_import(cx);
                        }
                    },
                )),
            );
        if busy {
            let text = figma.progress.lock().unwrap().clone();
            body = body.child(line(text.into(), tokens::text_muted()));
        } else if let Some((error, text)) = &figma.status {
            let color = if *error {
                tokens::text_error()
            } else {
                tokens::text_muted()
            };
            body = body.child(line(text.clone().into(), color));
        }
        if !figma.review.is_empty() {
            body = body.child(line(
                format!("Review ({})", figma.review.len()).into(),
                tokens::text_strong(),
            ));
            for (index, (reference, why)) in figma.review.iter().enumerate() {
                let reference = *reference;
                let name = self
                    .dom
                    .get(reference)
                    .map(|i| i.name().to_string())
                    .unwrap_or_default();
                body = body.child(
                    chrome::button(("figma-review", index), format!("{name}: {why}"), false)
                        .justify_start()
                        .on_click(cx.listener(move |shell, _, _, cx| shell.select(reference, cx))),
                );
            }
        }
        body = body.child(
            chrome::button("figma-close", "Close", false).on_click(cx.listener(
                |shell, _, _, cx| {
                    shell.ui.figma.open = false;
                    cx.notify();
                },
            )),
        );
        super::sidebar::sidebar(Some("Import from Figma".into()), "")
            .child(
                div()
                    .id("figma-panel")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(body),
            )
            .into_any_element()
    }
}

/// Blocking: signs the key in, then fetches, infers and uploads. The tree,
/// and the tokens again when the session refreshed them.
fn fetch(
    tokens: Tokens,
    link: &Link,
    progress: Arc<Mutex<String>>,
) -> Result<(Node, Option<Tokens>), String> {
    let key = ApiKey::from_env_or_config()
        .ok_or("No Open Cloud API key is set up. Add one from Home \u{203a} Manage key.")?;
    let client = Client::new(Some(key));
    let describe = super::super::freeze::describe;
    let user = client
        .introspect()
        .map_err(|e| describe(&e))?
        .authorized_user_id;
    let path = rbx_assets::cache_root()
        .map(|root| root.join("figma").join(format!("uploads-{user}.json")));
    let mut cache = path.map(Cache::open).unwrap_or_default();
    let mut session = Session::new(tokens);
    let tree = rbx_figma::import::import(
        &mut session,
        link,
        &mut cache,
        |name, png| {
            client
                .create_image_asset(name, "Imported from Figma by rbx-native.", user, png)
                .map_err(|e| describe(&e))
        },
        |line| *progress.lock().unwrap() = line,
    )?;
    let refreshed = session.refreshed.then(|| session.tokens().clone());
    Ok((tree, refreshed))
}

/// Builds `node` under `parent`: the editor's own GUI seeds first, then
/// what the design says. Collects each shaky guess into `review`.
fn materialize(
    dom: &mut WeakDom,
    database: &rbx_reflection::ReflectionDatabase,
    parent: Ref,
    node: Node,
    review: &mut Vec<(Ref, String)>,
) -> Ref {
    let reference = dom.new_instance(node.class, &node.name, Some(parent));
    for (name, text) in explorer::insert::gui_defaults(database, node.class) {
        let _ = crate::properties::edit::commit(dom, database, reference, name, text);
    }
    for (name, value) in node.properties {
        let _ = dom.set_property(reference, name, value);
    }
    if let Some(why) = node.review {
        review.push((reference, why));
    }
    for child in node.children {
        materialize(dom, database, reference, child, review);
    }
    reference
}
