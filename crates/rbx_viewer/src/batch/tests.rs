use std::fs;

use super::*;

#[test]
fn collect_finds_place_files_recursively_and_skips_everything_else() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    fs::create_dir_all(root.join("sub")).unwrap();
    fs::write(root.join("a.rbxl"), b"").unwrap();
    fs::write(root.join("b.RBXLX"), b"").unwrap();
    fs::write(root.join("readme.txt"), b"").unwrap();
    fs::write(root.join("sub/c.rbxm"), b"").unwrap();

    let found = collect(root).unwrap();
    let names: Vec<_> = found
        .iter()
        .map(|path| {
            path.strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();

    assert_eq!(names, vec!["a.rbxl", "b.RBXLX", "sub/c.rbxm"]);
}

#[test]
fn collect_treats_a_single_file_as_its_own_batch() {
    let dir = tempfile::tempdir().expect("temp dir");
    let file = dir.path().join("only.rbxl");
    fs::write(&file, b"").unwrap();

    assert_eq!(collect(&file).unwrap(), vec![file]);
}

#[test]
fn output_path_mirrors_the_input_directory_and_swaps_the_extension() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path().join("places");
    fs::create_dir_all(root.join("city")).unwrap();
    let input = root.join("city/downtown.rbxl");
    fs::write(&input, b"").unwrap();
    let out_dir = dir.path().join("out");

    assert_eq!(
        output_path(&root, &input, &out_dir),
        out_dir.join("city/downtown.png")
    );
}

#[test]
fn output_path_for_a_single_input_file_uses_just_its_name() {
    let dir = tempfile::tempdir().expect("temp dir");
    let input = dir.path().join("only.rbxl");
    fs::write(&input, b"").unwrap();
    let out_dir = dir.path().join("out");

    assert_eq!(
        output_path(&input, &input, &out_dir),
        out_dir.join("only.png")
    );
}

#[test]
fn is_place_file_matches_every_extension_case_insensitively_and_nothing_else() {
    assert!(is_place_file(Path::new("a.rbxl")));
    assert!(is_place_file(Path::new("a.RBXM")));
    assert!(is_place_file(Path::new("a.rbxlx")));
    assert!(is_place_file(Path::new("a.rbxmx")));
    assert!(!is_place_file(Path::new("a.png")));
    assert!(!is_place_file(Path::new("a")));
}
