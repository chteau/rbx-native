//! The images `Folder`s and `Model`s name in `ExplorerIconContent`, which
//! Studio draws as their Explorer icon. Each URI is loaded once per session
//! off the UI thread, through the same asset cache the viewport uses, and
//! the rows pick it up the moment it lands. One that fails to load keeps
//! the class icon, as Studio does.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::{AppContext, Context, RenderImage};

use super::Shell;
use crate::explorer;

#[derive(Default)]
pub(super) struct ExplorerIcons {
    /// `None` while loading, and for good once it failed.
    by_uri: HashMap<String, Option<Arc<RenderImage>>>,
}

impl ExplorerIcons {
    fn loaded(&self) -> HashMap<String, Arc<RenderImage>> {
        self.by_uri
            .iter()
            .filter_map(|(uri, image)| Some((uri.clone(), image.clone()?)))
            .collect()
    }
}

impl Shell {
    /// Puts every custom icon already loaded on the current Explorer and
    /// starts loading the ones it names for the first time. Called whenever
    /// the Explorer is rebuilt.
    pub(super) fn refresh_explorer_icons(&mut self, cx: &mut Context<Self>) {
        let wanted: Vec<String> = self
            .explorer
            .custom_icon_uris()
            .into_iter()
            .map(str::to_owned)
            .collect();
        if wanted.is_empty() {
            return;
        }
        for uri in &wanted {
            if self.explorer_icons.by_uri.contains_key(uri) {
                continue;
            }
            self.explorer_icons.by_uri.insert(uri.clone(), None);
            let uri = uri.clone();
            cx.spawn(async move |this, cx| {
                let wanted = uri.clone();
                let image = cx
                    .background_spawn(async move { explorer::custom_icon(&wanted) })
                    .await;
                let _ = this.update(cx, |shell, cx| match image {
                    Ok(image) => {
                        shell.explorer_icons.by_uri.insert(uri, Some(image));
                        shell.apply_explorer_icons();
                        cx.notify();
                    }
                    Err(why) => eprintln!("rbxstudio: Explorer icon {uri}: {why}"),
                });
            })
            .detach();
        }
        self.apply_explorer_icons();
    }

    fn apply_explorer_icons(&mut self) {
        let loaded = self.explorer_icons.loaded();
        if !loaded.is_empty() {
            self.explorer = Rc::new(self.explorer.with_custom_icons(&loaded));
        }
    }
}
