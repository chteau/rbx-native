use std::path::{Path, PathBuf};

use super::*;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `CREDITS.md` as `assets/credits.toml` describes it.
fn markdown() -> String {
    let mut out = String::from(
        "# Credits\n\n\
         RbxNative is built on other people's work. These are the third-party \
         fonts, icons, 3D models, data and programs it ships or fetches, and the \
         projects it took ideas from: who made them, under what licence, and what \
         RbxNative uses them for. The same list is in the app under Help \u{203a} \
         About RbxNative.\n\n\
         This file is generated from `assets/credits.toml`. Edit that, then run \
         `RBX_WRITE_CREDITS=1 cargo test -p rbx_studio credits`.\n\n\
         Every package carries the licence texts that have to travel with the \
         files in its `licenses/` folder, next to `THIRD-PARTY-LICENSES.html` \
         (the Rust crates compiled in) and `THIRD-PARTY-EMBEDDED.txt` (the \
         programs carried inside the editor).\n",
    );
    for group in groups() {
        out.push_str(&format!(
            "\n## {}\n\n| What | Owner | Licence | Credit required | Used for |\n|---|---|---|---|---|\n",
            group.title
        ));
        for credit in &group.entries {
            out.push_str(&format!(
                "| [{}]({}) | {} | {} | {} | {} |\n",
                credit.name,
                credit.url,
                credit.owner,
                credit.license,
                if credit.required { "yes" } else { "no" },
                credit.used_for
            ));
        }
    }
    out
}

/// A notice's repo path and its file name in a package's `licenses/`.
fn notice(credit: &Credit) -> Option<(&str, &str)> {
    credit.notice.as_deref()?.split_once(" as ")
}

#[test]
fn the_file_parses_into_groups_of_credits() {
    let groups = groups();
    assert!(groups.len() >= 5);
    for group in groups {
        assert!(!group.entries.is_empty(), "{} is empty", group.title);
        for credit in &group.entries {
            assert!(
                credit.url.starts_with("https://"),
                "{}: {}",
                credit.name,
                credit.url
            );
            for field in [&credit.owner, &credit.license, &credit.used_for] {
                assert!(
                    !field.trim().is_empty(),
                    "{} has an empty field",
                    credit.name
                );
                // A pipe would break CREDITS.md's table.
                assert!(!field.contains('|'), "{}: {field}", credit.name);
            }
        }
    }
}

/// `CREDITS.md` is exactly what the data file says; `RBX_WRITE_CREDITS=1`
/// rewrites it.
#[test]
fn credits_md_matches_the_data_file() {
    let path = repo().join("CREDITS.md");
    let expected = markdown();
    if std::env::var_os("RBX_WRITE_CREDITS").is_some() {
        std::fs::write(&path, &expected).unwrap();
    }
    // A Windows checkout has CRLF line ends.
    let actual = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert!(
        actual == expected,
        "CREDITS.md is out of date: run RBX_WRITE_CREDITS=1 cargo test -p rbx_studio credits"
    );
}

/// Every licence text the data names exists, under a distinct package
/// name, on the one line `.github/release/notices.sh` reads it from.
#[test]
fn every_notice_exists_and_the_packaging_script_can_read_it() {
    let lines: Vec<&str> = SOURCE
        .lines()
        .filter(|line| line.starts_with("notice = "))
        .collect();
    let notices: Vec<(&str, &str)> = groups()
        .iter()
        .flat_map(|group| &group.entries)
        .filter(|credit| credit.notice.is_some())
        .map(|credit| notice(credit).unwrap_or_else(|| panic!("{}: no \" as \"", credit.name)))
        .collect();
    assert_eq!(lines.len(), notices.len(), "a notice not on one line");
    let mut names = std::collections::HashSet::new();
    for (line, (file, name)) in lines.iter().zip(&notices) {
        assert_eq!(*line, format!("notice = \"{file} as {name}\""));
        assert!(
            !file.contains(' ') && !name.contains(' '),
            "{file} as {name}"
        );
        assert!(repo().join(file).is_file(), "{file} is missing");
        assert!(names.insert(*name), "{name} twice");
    }
    let script = std::fs::read_to_string(repo().join(".github/release/notices.sh")).unwrap();
    assert!(script.contains("assets/credits.toml") && script.contains("CREDITS.md"));
}

/// The fonts and graphics built into the program carry their licence text.
#[test]
fn every_shipped_font_and_graphic_has_its_licence() {
    let fonts = groups()
        .iter()
        .find(|group| group.title == "Fonts")
        .unwrap();
    let icons = groups()
        .iter()
        .find(|group| group.title == "Icons and graphics")
        .unwrap();
    for credit in fonts.entries.iter().chain(&icons.entries) {
        assert!(
            credit.required && credit.notice.is_some(),
            "{}",
            credit.name
        );
    }
}
