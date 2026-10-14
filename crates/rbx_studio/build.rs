//! Fetches the two programs this editor bundles for the target being built,
//! checks each against a pinned SHA-256, and leaves them in `$OUT_DIR` for
//! `include_bytes!`:
//!
//! - `luau-lsp.zip`, the `luau-lsp` release (see `src/luau_lsp`);
//! - `cloudflared.zip`, the `cloudflared` binary Play tunnels through (see
//!   `src/shell/play/cloudflared.rs`), re-packed deflated: the raw binary
//!   is ~40 MB, ~19 MB this way.
//!
//! Offline builds: set `RBX_LUAU_LSP_ZIP` / `RBX_CLOUDFLARED_ASSET` to an
//! already-downloaded copy of the same release asset; it is checked against
//! the same hash. A target with no release asset embeds an empty file, and
//! the editor falls back to the program on `PATH`.

use std::env;
use std::fs;
use std::io::{Read as _, Write as _};
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
    cloudflared();
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
        panic!("could not download {url}: {error} (set {LOCAL_VARIABLE} or {CLOUDFLARED_VARIABLE} to a local copy to build offline)")
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

/// Bumping this means updating every hash below from the release page
/// (`gh release view -R cloudflare/cloudflared <version> --json assets`).
const CLOUDFLARED_VERSION: &str = "2026.10.0";

/// `(target_os, target_arch, asset, sha256)`; the macOS assets are `.tgz`.
const CLOUDFLARED_ASSETS: &[(&str, &str, &str, &str)] = &[
    (
        "linux",
        "x86_64",
        "cloudflared-linux-amd64",
        "d33ff2d14475178d2012c2c56beba87389ac5ded27649519f198a7d3134a99db",
    ),
    (
        "linux",
        "aarch64",
        "cloudflared-linux-arm64",
        "e6422b9d4f72d3194bc5a38676f13667c06666523217b842a877d72a80b5ac08",
    ),
    (
        "macos",
        "x86_64",
        "cloudflared-darwin-amd64.tgz",
        "903845b81828c8cb3c5d13d816a2de71c06a3da5785469df8eb0e1b736d92f9f",
    ),
    (
        "macos",
        "aarch64",
        "cloudflared-darwin-arm64.tgz",
        "a2f79ff7b9420aa537d74af239f376da170bbabeb529aec416002adac6a72e70",
    ),
    (
        "windows",
        "x86_64",
        "cloudflared-windows-amd64.exe",
        "86aee4017b26625cee8484c113558f48effa4cd47f7aa05fcf425604e5d2b23c",
    ),
];

const CLOUDFLARED_VARIABLE: &str = "RBX_CLOUDFLARED_ASSET";

/// The name the binary has inside `cloudflared.zip`.
const CLOUDFLARED_ENTRY: &str = "cloudflared";

fn cloudflared() {
    println!("cargo:rerun-if-env-changed={CLOUDFLARED_VARIABLE}");
    println!("cargo:rustc-env=CLOUDFLARED_VERSION={CLOUDFLARED_VERSION}");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    let out = out_dir.join("cloudflared.zip");
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let Some(&(_, _, asset, sha256)) = CLOUDFLARED_ASSETS
        .iter()
        .find(|(o, a, ..)| *o == os && *a == arch)
    else {
        println!("cargo:warning=no cloudflared release for {os}/{arch}; Play will look on PATH");
        fs::write(&out, []).expect("OUT_DIR is writable");
        return;
    };

    // The zip is rebuilt only when the asset it came from changes: a marker
    // next to it records which hash it was packed from.
    let marker = out_dir.join("cloudflared.sha256");
    if out.exists() && fs::read_to_string(&marker).is_ok_and(|done| done == sha256) {
        return;
    }
    let bytes = match env::var_os(CLOUDFLARED_VARIABLE) {
        Some(path) => fs::read(&path).unwrap_or_else(|error| {
            panic!(
                "{CLOUDFLARED_VARIABLE}={}: {error}",
                PathBuf::from(&path).display()
            )
        }),
        None => download(&format!(
            "https://github.com/cloudflare/cloudflared/releases/download/{CLOUDFLARED_VERSION}/{asset}"
        )),
    };
    assert_eq!(
        hex_sha256(&bytes),
        sha256,
        "{asset} does not match the pinned hash for cloudflared {CLOUDFLARED_VERSION}"
    );
    let binary = if asset.ends_with(".tgz") {
        untar_one(&bytes, "cloudflared")
            .unwrap_or_else(|| panic!("{asset} holds no `cloudflared` file"))
    } else {
        bytes
    };

    let mut zipped = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(9))
        .large_file(false);
    zipped
        .start_file(CLOUDFLARED_ENTRY, options)
        .expect("zip entry");
    zipped.write_all(&binary).expect("zip write");
    let zipped = zipped.finish().expect("zip finish").into_inner();
    fs::write(&out, zipped).expect("OUT_DIR is writable");
    fs::write(&marker, sha256).expect("OUT_DIR is writable");
}

/// The file named `name` (any directory) inside a gzipped tar. Plain
/// ustar: 512-byte headers, size in octal at 124..136, data padded to 512.
fn untar_one(tgz: &[u8], name: &str) -> Option<Vec<u8>> {
    let mut tar = Vec::new();
    flate2::read::GzDecoder::new(tgz)
        .read_to_end(&mut tar)
        .ok()?;
    let mut at = 0;
    while at + 512 <= tar.len() {
        let header = &tar[at..at + 512];
        if header.iter().all(|&b| b == 0) {
            return None;
        }
        let field = |range: std::ops::Range<usize>| {
            let raw = &header[range];
            let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
            String::from_utf8_lossy(&raw[..end]).trim().to_owned()
        };
        let path = field(0..100);
        let size = usize::from_str_radix(&field(124..136), 8).ok()?;
        let data = at + 512;
        if path.rsplit('/').next() == Some(name) && header[156] != b'5' {
            return tar.get(data..data + size).map(<[u8]>::to_vec);
        }
        at = data + size.div_ceil(512) * 512;
    }
    None
}
