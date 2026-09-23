//! Everything the dock asks the registry for: the Featured cards, a
//! search, and one package's metadata. Each runs on the background
//! executor and lands back in `Shell::wally` only if nothing newer was
//! asked for since.

use gpui_kit::Context;

use crate::command_bar::Feedback;

use crate::wally_client::{self, Listing};

use super::{package_id, PackageId, Remote, Shell, LOG_SOURCE, RESULT_LIMIT, SEARCH_DEBOUNCE};

impl Shell {
    /// Why the registry couldn't answer, in Output; the page itself only
    /// says that it couldn't.
    fn wally_unreachable(&mut self, message: String) {
        self.output.push(
            LOG_SOURCE,
            Feedback::Warning(format!("Couldn't reach the Wally registry: {message}")),
        );
    }

    /// A featured card's click: its `scope/name` becomes the query, so the
    /// result card with the realm switch, the version picker and Add
    /// comes up for it.
    pub(in crate::shell) fn wally_search_for(
        &mut self,
        query: String,
        window: &mut gpui_kit::Window,
        cx: &mut Context<Self>,
    ) {
        self.wally_query
            .update(cx, |state, cx| state.set_value(query, window, cx));
        self.wally_query_changed(cx);
    }

    /// The Home page's Featured cards, fetched once; a failure stays until
    /// "Try again" ([`Shell::wally_retry`]).
    pub(in crate::shell) fn wally_load_featured(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.wally.featured, Remote::Idle) {
            return;
        }
        self.wally.featured = Remote::Loading;
        cx.spawn(async move |shell, cx| {
            let fetched = cx
                .background_executor()
                .spawn(async { fetch_featured() })
                .await;
            let _ = shell.update(cx, |shell, cx| {
                shell.wally.featured = match fetched {
                    Ok(listings) => Remote::Ready(listings),
                    Err(message) => {
                        shell.wally_unreachable(message);
                        Remote::Failed
                    }
                };
                cx.notify();
            });
        })
        .detach();
    }

    /// Re-runs whichever request failed: the search when there is one,
    /// otherwise Featured.
    pub(in crate::shell) fn wally_retry(&mut self, cx: &mut Context<Self>) {
        if !self.wally.query.is_empty() {
            self.wally.results = Remote::Idle;
            self.wally_query_changed(cx);
            return;
        }
        self.wally.featured = Remote::Idle;
        self.wally_load_featured(cx);
    }

    /// The search field: debounced (~400ms, the same shape `shell::
    /// scripts`'s commit debounce and `shell::argon_sync`'s write debounce
    /// already use), a background-thread `package-search` call, results
    /// swapped in only if nothing newer has been typed since. Each result
    /// then gets its metadata, for its realm.
    pub(in crate::shell) fn wally_query_changed(&mut self, cx: &mut Context<Self>) {
        self.wally.generation = self.wally.generation.wrapping_add(1);
        let generation = self.wally.generation;
        let query = self.wally_query.read(cx).value().trim().to_string();
        self.wally.query = query.clone();
        if query.is_empty() {
            self.wally.results = Remote::Idle;
            cx.notify();
            return;
        }
        self.wally.results = Remote::Loading;
        cx.notify();
        cx.spawn(async move |shell, cx| {
            cx.background_executor().timer(SEARCH_DEBOUNCE).await;
            if shell
                .read_with(cx, |shell, _| shell.wally.generation)
                .unwrap_or(generation)
                != generation
            {
                return;
            }
            let results = cx
                .background_executor()
                .spawn(async move { wally_client::search(&query) })
                .await;
            let _ = shell.update(cx, |shell, cx| {
                if shell.wally.generation != generation {
                    return;
                }
                match results {
                    Ok(mut results) => {
                        results.truncate(RESULT_LIMIT);
                        let ids = results
                            .iter()
                            .map(|result| package_id(&result.scope, &result.name))
                            .collect();
                        shell.wally.results = Remote::Ready(results);
                        shell.wally_fetch_metadata(ids, cx);
                    }
                    Err(message) => {
                        shell.wally_unreachable(message);
                        shell.wally.results = Remote::Failed;
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// One metadata fetch per package not already known or in flight.
    pub(in crate::shell) fn wally_fetch_metadata(
        &mut self,
        ids: Vec<PackageId>,
        cx: &mut Context<Self>,
    ) {
        for id in ids {
            if self.wally.metadata.contains_key(&id) {
                continue;
            }
            self.wally.metadata.insert(id.clone(), Remote::Loading);
            cx.spawn(async move |shell, cx| {
                let fetched = {
                    let id = id.clone();
                    cx.background_executor()
                        .spawn(async move { wally_client::metadata(&id.0, &id.1) })
                        .await
                };
                let _ = shell.update(cx, |shell, cx| {
                    let state = match fetched {
                        Ok(listing) => Remote::Ready(listing),
                        Err(message) => {
                            shell.wally_unreachable(message);
                            Remote::Failed
                        }
                    };
                    shell.wally.metadata.insert(id, state);
                    cx.notify();
                });
            })
            .detach();
        }
    }
}

/// The Featured list, one metadata call per package, in the list's order.
/// A package the registry can't describe is left out; only when none can
/// be does the whole page fail, with the first error.
fn fetch_featured() -> Result<Vec<Listing>, String> {
    let mut listings = Vec::new();
    let mut first_error = None;
    for (scope, name) in wally_client::FEATURED {
        match wally_client::metadata(scope, name) {
            Ok(listing) => listings.push(listing),
            Err(message) => {
                first_error.get_or_insert(message);
            }
        }
    }
    if listings.is_empty() {
        return Err(first_error.unwrap_or_else(|| "nothing featured".to_owned()));
    }
    Ok(listings)
}
