//! Read-only views of the parked plugin instances for the editor.

use rbx_dom::{Instance, Variant, WeakDom};

use super::PluginHost;
use crate::plugin::api::UiId;
use crate::plugin::lot::subtree;

pub(super) fn widget_dom(host: &PluginHost, widget: UiId) -> Option<WeakDom> {
    let (root, title) = {
        let shared = host.shared.borrow();
        let w = shared.ui.widgets.iter().find(|w| w.id == widget)?;
        (w.root, w.title.clone())
    };
    let children = host.lot.parked.get(root)?.children().to_vec();
    let mut copy = host.lot.parked.clone();
    for &child in &children {
        copy.set_parent(child, None);
    }
    let mut out = WeakDom::new();
    out.insert(Instance::new(root, "ScreenGui", title));
    copy.move_subtrees(&children, &mut out);
    for &child in &children {
        out.set_parent(child, Some(root));
    }
    out.take_changes();
    Some(out)
}

pub(super) fn adornments(host: &PluginHost) -> Vec<Instance> {
    let parked = &host.lot.parked;
    let db = &host.database;
    parked
        .root_refs()
        .iter()
        .flat_map(|&root| subtree(parked, root))
        .filter_map(|referent| {
            let instance = parked.get(referent)?;
            let class = instance.class();
            let is_3d = matches!(class, "Handles" | "ArcHandles" | "SelectionBox")
                || db.is_subclass_of(class, "GuiBase3d");
            let adorned = matches!(instance.properties().get("Adornee"), Some(Variant::Ref(_)));
            if !is_3d || !adorned {
                return None;
            }
            let mut copy = Instance::new(referent, class, instance.name());
            copy.properties_mut()
                .extend(instance.properties().iter().map(|(k, v)| (k.clone(), v.clone())));
            Some(copy)
        })
        .collect()
}
