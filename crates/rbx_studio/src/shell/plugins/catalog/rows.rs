//! The rows the Popular tab shows and the state each one is in.

use std::sync::Arc;

use gpui_kit::RenderImage;

use super::super::discover;
use super::{installs, list, Load};
use crate::shell::Shell;

/// One catalog row, whatever list it came from.
pub(in crate::shell::plugins) struct Row {
    pub(super) id: u64,
    pub(super) name: String,
    pub(super) creator: String,
    pub(super) blurb: String,
    pub(super) votes: Option<u64>,
    pub(super) verified: bool,
    pub(super) icon: Option<Arc<RenderImage>>,
    pub(super) state: State,
    pub(super) failure: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::shell::plugins) enum State {
    Available,
    Installed,
    UpdateAvailable,
    Installing,
}

/// The first line of a store description, cut short: a fallback blurb for
/// a search hit, which has none of ours.
fn first_line(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty());
    let line = line.unwrap_or("");
    match line.char_indices().nth(120) {
        Some((cut, _)) => format!("{}\u{2026}", line[..cut].trim_end()),
        None => line.to_owned(),
    }
}

impl Shell {
    /// The rows the Popular tab shows: the search's hits while a search is
    /// on, else the verified list, most voted first once votes are in.
    pub(super) fn catalog_rows(&self) -> Vec<Row> {
        let catalog = &self.plugins.catalog;
        let ids: Vec<u64> = if catalog.search.load == Load::Idle {
            let mut ids: Vec<u64> = list::VERIFIED.iter().map(|entry| entry.id).collect();
            ids.sort_by_key(|id| {
                std::cmp::Reverse(catalog.items.get(id).map_or(0, |item| item.up_votes))
            });
            ids
        } else {
            catalog.search.ids.clone()
        };
        let dir = discover::own_dir();
        ids.into_iter()
            .map(|id| {
                let item = catalog.items.get(&id);
                let verified = list::verified(id);
                let record = dir
                    .as_deref()
                    .and_then(|dir| catalog.installs.live(dir, id));
                let state = match record {
                    _ if catalog.busy.contains(&id) => State::Installing,
                    Some(record)
                        if item.is_some_and(|item| installs::has_update(record, &item.updated)) =>
                    {
                        State::UpdateAvailable
                    }
                    Some(_) => State::Installed,
                    None => State::Available,
                };
                Row {
                    id,
                    name: verified
                        .map(|entry| entry.name.to_owned())
                        .or_else(|| item.map(|item| item.name.clone()))
                        .unwrap_or_default(),
                    creator: item
                        .map(|item| item.creator_name.clone())
                        .or_else(|| verified.map(|entry| entry.creator.to_owned()))
                        .unwrap_or_default(),
                    blurb: verified
                        .map(|entry| entry.blurb.to_owned())
                        .or_else(|| item.map(|item| first_line(&item.description)))
                        .unwrap_or_default(),
                    votes: item.map(|item| item.up_votes),
                    verified: verified.is_some(),
                    icon: catalog.icons.get(&id).cloned().flatten(),
                    state,
                    failure: catalog.failures.get(&id).cloned(),
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::super::list;
    use super::first_line;

    #[test]
    fn first_line_skips_blanks_and_cuts_long_lines() {
        assert_eq!(first_line("\n\n  Smooth arcs.  \nMore"), "Smooth arcs.");
        let long = "a".repeat(200);
        assert_eq!(first_line(&long).chars().count(), 121);
        assert_eq!(first_line(""), "");
    }

    #[test]
    fn verified_ids_are_unique_and_described() {
        let mut ids: Vec<u64> = list::VERIFIED.iter().map(|entry| entry.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), list::VERIFIED.len());
        for entry in list::VERIFIED {
            assert!(!entry.name.is_empty() && !entry.creator.is_empty());
            assert!(entry.blurb.len() <= 80, "{} blurb too long", entry.name);
        }
    }
}
