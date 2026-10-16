//! Keeps one overlay [`Widget`] per `ScreenGui` a plugin parented to its
//! `CoreGui`. Runs against the merged DOM.

use std::collections::HashSet;

use rbx_dom::{Ref, Variant};

use super::PluginHost;
use crate::plugin::api::{Widget, WidgetKind};

pub(super) fn sync(host: &mut PluginHost) {
    let mut found = Vec::new();
    {
        let dom = host.dom.borrow();
        for env in host.plugins.iter().filter_map(|p| p.env.as_ref()) {
            let Some(core) = dom.get(env.core_gui) else {
                continue;
            };
            for &child in core.children() {
                let Some(gui) = dom.get(child).filter(|i| i.class() == "ScreenGui") else {
                    continue;
                };
                let enabled =
                    !matches!(gui.properties().get("Enabled"), Some(Variant::Bool(false)));
                let order = match gui.properties().get("DisplayOrder") {
                    Some(Variant::Int32(v)) => *v,
                    _ => 0,
                };
                found.push((env.plugin, child, gui.name().to_string(), enabled, order));
            }
        }
    }
    let live: HashSet<Ref> = found.iter().map(|f| f.1).collect();
    let mut shared = host.shared.borrow_mut();
    let mut changed = false;
    let before = shared.ui.widgets.len();
    let mut gone = Vec::new();
    shared.ui.widgets.retain(|w| {
        let stale = matches!(w.kind, WidgetKind::Overlay { .. }) && !live.contains(&w.root);
        if stale {
            gone.push(w.root);
        }
        !stale
    });
    for root in gone {
        shared.widget_roots.remove(&root);
    }
    changed |= shared.ui.widgets.len() != before;
    for (plugin, root, name, enabled, display_order) in found {
        let kind = WidgetKind::Overlay { display_order };
        let existing = shared
            .ui
            .widgets
            .iter_mut()
            .find(|w| w.root == root && matches!(w.kind, WidgetKind::Overlay { .. }));
        match existing {
            Some(w) => {
                if w.title != name || w.enabled != enabled || w.kind != kind {
                    w.key = name.clone();
                    w.title = name;
                    w.enabled = enabled;
                    w.kind = kind;
                    w.revision += 1;
                    changed = true;
                }
            }
            None => {
                let id = shared.next_id();
                shared.ui.widgets.push(Widget {
                    id,
                    plugin,
                    key: name.clone(),
                    title: name,
                    enabled,
                    root,
                    kind,
                    revision: 0,
                });
                shared.widget_roots.insert(root);
                changed = true;
            }
        }
    }
    shared.ui_changed |= changed;
}
