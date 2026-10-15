//! Instances named by their full path from `game`: the names from the
//! service down, joined with `.`, a `.` or `\` inside a name escaped with
//! `\` (`Workspace.Model\.v2.Part`). `game` itself is left out.
//!
//! A path is not an identity: two siblings with the same name have the same
//! path, and [`resolve`] (like `Paths.resolve` in the plugin, which uses
//! `FindFirstChild`) takes the first one in child order. Studio and
//! rbx-native may not agree on that order for duplicates, so a duplicate
//! name can point the two editors at different instances.

use rbx_dom::{Ref, WeakDom};

/// `name` with `\` → `\\` and `.` → `\.`.
pub fn escape(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if c == '\\' || c == '.' {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// `names` escaped and joined with `.`.
pub fn join<S: AsRef<str>>(names: &[S]) -> String {
    names
        .iter()
        .map(|n| escape(n.as_ref()))
        .collect::<Vec<_>>()
        .join(".")
}

/// The names in `path`, unescaped; `None` for an empty path, a trailing lone
/// `\`, or `\` before anything but `.` or `\`.
pub fn split(path: &str) -> Option<Vec<String>> {
    if path.is_empty() {
        return None;
    }
    let mut names = Vec::new();
    let mut current = String::new();
    let mut chars = path.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some(e @ ('.' | '\\')) => current.push(e),
                _ => return None,
            },
            '.' => names.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    names.push(current);
    Some(names)
}

/// `referent`'s path: its name and every ancestor's up to (and including)
/// the root-level instance (a service in a place). `None` if it is not in
/// `dom`.
pub fn path_of(dom: &WeakDom, referent: Ref) -> Option<String> {
    let mut names = Vec::new();
    let mut current = referent;
    loop {
        names.push(dom.get(current)?.name());
        match dom.parent(current) {
            Some(parent) => current = parent,
            None => break,
        }
    }
    names.reverse();
    Some(join(&names))
}

/// The instance `path` names: the first root-level instance with the first
/// name, then the first child with each next name.
pub fn resolve(dom: &WeakDom, path: &str) -> Option<Ref> {
    let names = split(path)?;
    let mut level: Vec<Ref> = dom.root_refs().to_vec();
    let mut found = None;
    for name in &names {
        let next = level
            .iter()
            .copied()
            .find(|r| dom.get(*r).is_some_and(|i| i.name() == name))?;
        level = dom.get(next)?.children().to_vec();
        found = Some(next);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_cases() {
        let text = include_str!("../../../plugins/team-presence/fixtures/paths.json");
        let json: serde_json::Value = serde_json::from_str(text).unwrap();
        for case in json["valid"].as_array().unwrap() {
            let names: Vec<String> = case["names"]
                .as_array()
                .unwrap()
                .iter()
                .map(|n| n.as_str().unwrap().to_string())
                .collect();
            let path = case["path"].as_str().unwrap();
            assert_eq!(join(&names), path);
            assert_eq!(split(path), Some(names));
        }
        for bad in json["invalid"].as_array().unwrap() {
            assert_eq!(split(bad.as_str().unwrap()), None);
        }
    }

    #[test]
    fn path_of_and_resolve_round_trip() {
        let mut dom = WeakDom::new();
        let ws = dom.new_instance("Workspace", "Workspace", None);
        let model = dom.new_instance("Model", "Model.v2", Some(ws));
        let first = dom.new_instance("Part", "Part", Some(model));
        let second = dom.new_instance("Part", "Part", Some(model));
        assert_eq!(path_of(&dom, first).unwrap(), "Workspace.Model\\.v2.Part");
        assert_eq!(path_of(&dom, second).unwrap(), "Workspace.Model\\.v2.Part");
        assert_eq!(resolve(&dom, "Workspace.Model\\.v2.Part"), Some(first));
        assert_eq!(resolve(&dom, "Workspace"), Some(ws));
        assert_eq!(resolve(&dom, "Workspace.Nope"), None);
        assert_eq!(resolve(&dom, ""), None);
    }
}
