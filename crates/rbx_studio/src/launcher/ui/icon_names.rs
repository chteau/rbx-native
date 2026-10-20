//! Every glyph name written as a literal in the source must be a file in
//! the kit's icon catalogue: a missing one draws nothing but still keeps its
//! slot, which pushes the label beside it off centre.

use std::path::{Path, PathBuf};

use super::icon_exists;

/// Calls whose glyph argument is a literal, with the argument's index.
const CALLS: [(&str, usize); 4] = [
    ("icon", 0),
    ("icon_button", 1),
    ("dialog_glyph", 0),
    ("status_dot", 0),
];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The top-level arguments of the call whose `(` is just before `rest`.
fn arguments(rest: &str) -> Vec<String> {
    let mut args = vec![String::new()];
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for c in rest.chars() {
        if in_string {
            args.last_mut().unwrap().push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                args.last_mut().unwrap().push(c);
            }
            '(' | '[' | '{' => {
                depth += 1;
                args.last_mut().unwrap().push(c);
            }
            ')' | ']' | '}' => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
                args.last_mut().unwrap().push(c);
            }
            ',' if depth == 0 => args.push(String::new()),
            _ => args.last_mut().unwrap().push(c),
        }
    }
    args
}

fn plain_name(arg: &str) -> Option<&str> {
    let name = arg.trim().strip_prefix('"')?.strip_suffix('"')?;
    let plain = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    plain.then_some(name)
}

fn glyph_names(source: &str) -> Vec<String> {
    let code: String = source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut names = Vec::new();
    for (call, index) in CALLS {
        let needle = format!("{call}(");
        for (at, _) in code.match_indices(&needle) {
            let before = code[..at].chars().next_back();
            if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                continue;
            }
            let args = arguments(&code[at + needle.len()..]);
            if let Some(name) = args.get(index).and_then(|arg| plain_name(arg)) {
                names.push(name.to_owned());
            }
        }
    }
    names
}

#[test]
fn every_literal_glyph_is_in_the_catalogue() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&root, &mut files);
    let this = Path::new(file!()).file_name().unwrap().to_owned();
    let mut missing = Vec::new();
    for file in files {
        if file.file_name() == Some(this.as_os_str()) {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(&file) else {
            continue;
        };
        for name in glyph_names(&source) {
            if !icon_exists(&name) {
                missing.push(format!("{name} ({})", file.display()));
            }
        }
    }
    assert!(missing.is_empty(), "missing icons:\n{}", missing.join("\n"));
}

#[test]
fn parser_reads_only_plain_literals() {
    let source = r#"
        ui::icon_button(id, "trash", "Delete", w, true);
        icon("a-b", 1.); icon(name, 1.); icon(&format!("x{}", 1), 1.);
        fn icon(name: &'static str) {}
        my_icon("nope");
    "#;
    assert_eq!(glyph_names(source), vec!["a-b", "trash"]);
}
