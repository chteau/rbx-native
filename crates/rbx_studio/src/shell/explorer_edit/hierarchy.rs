//! The Explorer's Copy Path and Hierarchy rows, measured in real Studio on
//! 2026-11-03.
//!
//! Copy Path writes `game.Workspace.Baseplate`: a service by its name
//! (`game.ReplicatedStorage`, never `GetService`), and a name that is not an
//! identifier in brackets (`game.Workspace["my part"]`). Studio writes a Luau
//! keyword name bare (`game.Workspace.end`), which does not parse, so
//! keywords are bracketed here. Expand All opens the row and everything under
//! it; Collapse All closes the row and everything under it, so reopening it
//! later shows its children closed.

use gpui_kit::*;
use rbx_dom::{Ref, WeakDom};

use super::Shell;

const KEYWORDS: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "if", "in", "local",
    "nil", "not", "or", "repeat", "return", "then", "true", "until", "while", "continue",
];

fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn escape(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\{:03}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

/// The Luau expression Studio's Copy Path would write for `reference`, or
/// `None` when it is not in the DOM. The DOM has no DataModel instance: its
/// roots are the services, so `game` is implied above them.
pub(crate) fn lua_path(dom: &WeakDom, reference: Ref) -> Option<String> {
    let mut names = Vec::new();
    let mut at = Some(reference);
    while let Some(current) = at {
        names.push(dom.get(current)?.name().to_owned());
        at = dom.parent(current);
    }
    let mut path = String::from("game");
    for name in names.iter().rev() {
        if is_identifier(name) && !KEYWORDS.contains(&name.as_str()) {
            path.push('.');
            path.push_str(name);
        } else {
            path.push_str("[\"");
            path.push_str(&escape(name));
            path.push_str("\"]");
        }
    }
    Some(path)
}

/// Whether the row has anything for Expand All and Collapse All to act on.
pub(super) fn has_listed_children(dom: &WeakDom, target: Ref) -> bool {
    dom.get(target)
        .is_some_and(|instance| !instance.children().is_empty())
}

impl Shell {
    pub(super) fn copy_selected_paths(&mut self, cx: &mut Context<Self>) {
        let joined = self
            .selected_all()
            .iter()
            .copied()
            .filter_map(|reference| lua_path(&self.dom, reference))
            .collect::<Vec<_>>()
            .join("\n");
        if !joined.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(joined));
        }
    }

    pub(super) fn select_children_of(&mut self, target: Ref, cx: &mut Context<Self>) {
        let Some(instance) = self.dom.get(target) else {
            return;
        };
        let children: Vec<Ref> = instance
            .children()
            .iter()
            .copied()
            .filter(|&child| self.explorer.item(child).is_some())
            .collect();
        if !children.is_empty() {
            self.reselect(children, cx);
        }
    }

    pub(super) fn set_subtree_expanded(
        &mut self,
        target: Ref,
        expanded: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(item) = self.explorer.item(target) else {
            return;
        };
        self.tree.update(cx, |tree, cx| {
            tree.set_subtree_expanded(&item.id, expanded, cx)
        });
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use rbx_dom::WeakDom;

    use super::lua_path;

    fn path_of(names: &[&str]) -> String {
        let mut dom = WeakDom::new();
        let mut parent = None;
        let mut last = None;
        for (i, name) in names.iter().enumerate() {
            let class = if i == 0 { "Workspace" } else { "Folder" };
            let made = dom.new_instance(class, name, parent);
            parent = Some(made);
            last = Some(made);
        }
        lua_path(&dom, last.unwrap()).unwrap()
    }

    #[test]
    fn a_service_is_named_directly() {
        assert_eq!(path_of(&["Workspace"]), "game.Workspace");
    }

    #[test]
    fn nested_identifiers_are_dotted() {
        assert_eq!(
            path_of(&["Workspace", "Model", "Part_1"]),
            "game.Workspace.Model.Part_1"
        );
    }

    #[test]
    fn a_space_is_bracketed() {
        assert_eq!(
            path_of(&["Workspace", "my part"]),
            "game.Workspace[\"my part\"]"
        );
    }

    #[test]
    fn a_keyword_is_bracketed() {
        assert_eq!(path_of(&["Workspace", "end"]), "game.Workspace[\"end\"]");
    }

    #[test]
    fn a_leading_digit_is_bracketed() {
        assert_eq!(path_of(&["Workspace", "1st"]), "game.Workspace[\"1st\"]");
    }

    #[test]
    fn quote_and_backslash_are_escaped() {
        assert_eq!(
            path_of(&["Workspace", "a\"b\\c"]),
            "game.Workspace[\"a\\\"b\\\\c\"]"
        );
    }

    #[test]
    fn a_newline_is_escaped() {
        assert_eq!(path_of(&["Workspace", "a\nb"]), "game.Workspace[\"a\\nb\"]");
    }

    #[test]
    fn a_control_character_is_three_digits() {
        assert_eq!(
            path_of(&["Workspace", "a\u{1}b"]),
            "game.Workspace[\"a\\001b\"]"
        );
    }
}
