//! Fetches the `luau-lsp` release this editor bundles (see `src/luau_lsp`)
//! for the target being built, checks it against a pinned SHA-256, and
//! leaves it at `$OUT_DIR/luau-lsp.zip` for `include_bytes!`.
//!
//! Offline builds: set `RBX_LUAU_LSP_ZIP` to an already-downloaded copy of
//! the same asset; it is checked against the same hash. A target with no
//! release asset embeds an empty file, and the editor falls back to a
//! `luau-lsp` on `PATH`.

use std::env;
use std::fs;
use std::io::Read as _;
use std::path::PathBuf;

use sha2::{Digest as _, Sha256};

/// Bumping this means updating every hash below from the release page
/// (`gh release view -R JohnnyMorganz/luau-lsp <version> --json assets`).
const VERSION: &str = "1.70.0";

/// `(target_os, target_arch, asset, sha256)`. The macOS asset is universal.
const ASSETS: &[(&str, &str, &str, &str)] = &[
    (
        "linux",
        "x86_64",
        "luau-lsp-linux-x86_64.zip",
        "4ff08890ea0d4b6d9de25fdff1a4c87e0dc9f2e45d782c894e83475e51d55813",
    ),
    (
        "linux",
        "aarch64",
        "luau-lsp-linux-arm64.zip",
        "24c185fb7c4fe5feae9c9c95de83ae3eb1cd284766ce2d151aea75235501034a",
    ),
    (
        "macos",
        "x86_64",
        "luau-lsp-macos.zip",
        "b0491a64f441a37f187b34f01c932ddcfc7f3603cefd043cd5a385da33ba7de8",
    ),
    (
        "macos",
        "aarch64",
        "luau-lsp-macos.zip",
        "b0491a64f441a37f187b34f01c932ddcfc7f3603cefd043cd5a385da33ba7de8",
    ),
    (
        "windows",
        "x86_64",
        "luau-lsp-win64.zip",
        "26e6d32069cb5dd74f06bbdc2f3033d3ba66da76d3dbdfc3dcc2b085ddaa274a",
    ),
];

const LOCAL_VARIABLE: &str = "RBX_LUAU_LSP_ZIP";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed={LOCAL_VARIABLE}");
    println!("cargo:rustc-env=LUAU_LSP_VERSION={VERSION}");

    let out = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR")).join("luau-lsp.zip");
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let Some(&(_, _, asset, sha256)) = ASSETS.iter().find(|(o, a, ..)| *o == os && *a == arch)
    else {
        println!("cargo:warning=no luau-lsp release for {os}/{arch}; the editor will look on PATH");
        fs::write(&out, []).expect("OUT_DIR is writable");
        return;
    };

    // Already fetched and intact: a rebuild costs nothing.
    if fs::read(&out).is_ok_and(|bytes| hex_sha256(&bytes) == sha256) {
        return;
    }
    let bytes = match env::var_os(LOCAL_VARIABLE) {
        Some(path) => fs::read(&path).unwrap_or_else(|error| {
            panic!(
                "{LOCAL_VARIABLE}={}: {error}",
                PathBuf::from(&path).display()
            )
        }),
        None => download(&format!(
            "https://github.com/JohnnyMorganz/luau-lsp/releases/download/{VERSION}/{asset}"
        )),
    };
    let actual = hex_sha256(&bytes);
    assert_eq!(
        actual, sha256,
        "{asset} does not match the pinned hash for luau-lsp {VERSION}"
    );
    fs::write(&out, bytes).expect("OUT_DIR is writable");
}

fn download(url: &str) -> Vec<u8> {
    let failed = |error: &dyn std::fmt::Display| -> ! {
        panic!("could not download {url}: {error} (set {LOCAL_VARIABLE} to a local copy to build offline)")
    };
    let mut response = ureq::get(url).call().unwrap_or_else(|error| failed(&error));
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .read_to_end(&mut bytes)
        .unwrap_or_else(|error| failed(&error));
    bytes
}

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
