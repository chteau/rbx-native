//! Compiles `src/shim.cpp` against the Luau sources that `mlua-sys` builds.
//!
//! The shim uses `BytecodeBuilder` and the code generator's C++ API, whose
//! class layouts have to match the compiled libraries exactly, so the headers
//! come from the same `luau0-src` package cargo resolved for mlua-sys, found
//! through `cargo metadata`. The defines below mirror `luau0-src`'s own.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=src/shim.cpp");
    let luau = luau_source_dir().join("luau");

    let mut build = cc::Build::new();
    build
        .cpp(true)
        .file("src/shim.cpp")
        .warnings(false)
        .define("LUAI_MAXCSTACK", "1000000")
        .define("LUA_VECTOR_SIZE", "3")
        .define("LUA_API", "extern \"C\"")
        .define("LUACODE_API", "extern \"C\"")
        .define("LUACODEGEN_API", "extern \"C\"");
    if cfg!(debug_assertions) {
        build.define("LUAU_ENABLE_ASSERT", None);
    }
    for dir in ["Common", "Ast", "Bytecode", "Compiler", "CodeGen", "VM"] {
        build.include(luau.join(dir).join("include"));
    }
    if build.get_compiler().is_like_msvc() {
        build.flag("/std:c++17").flag("/EHsc");
    } else {
        build.flag("-std=c++17");
    }
    build.compile("rbx_bytecode_shim");
}

/// The `luau0-src` package's directory, the one `mlua-sys` builds with.
fn luau_source_dir() -> PathBuf {
    if let Some(dir) = env::var_os("RBX_LUAU_SOURCE") {
        return PathBuf::from(dir);
    }
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("Cargo.toml");
    // Offline needs every package in the graph on disk, and a build only
    // fetched the ones for its own target (CI has no accesskit_macos on
    // Linux), so ask for just that target's graph.
    let target = env::var("TARGET").unwrap();
    let output = Command::new(cargo)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--offline",
            "--filter-platform",
            &target,
            "--manifest-path",
        ])
        .arg(&manifest)
        .output()
        .expect("cargo metadata runs");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("cargo metadata prints JSON");

    // The `luau0-src` mlua-sys depends on, in case the graph holds two.
    let packages = metadata["packages"].as_array().expect("packages");
    let id_of = |name: &str| {
        packages
            .iter()
            .find(|p| p["name"] == name)
            .and_then(|p| p["id"].as_str())
    };
    let wanted = metadata["resolve"]["nodes"]
        .as_array()
        .and_then(|nodes| {
            let sys = id_of("mlua-sys")?;
            let node = nodes.iter().find(|n| n["id"] == sys)?;
            node["deps"]
                .as_array()?
                .iter()
                .find(|d| d["name"] == "luau0_src")
                .and_then(|d| d["pkg"].as_str())
        })
        .or_else(|| id_of("luau0-src"))
        .expect("luau0-src is in the dependency graph");
    let package = packages
        .iter()
        .find(|p| p["id"] == wanted)
        .expect("luau0-src package");
    let manifest_path = PathBuf::from(package["manifest_path"].as_str().expect("manifest_path"));
    let dir = manifest_path.parent().expect("package dir").to_path_buf();
    println!("cargo:rerun-if-env-changed=RBX_LUAU_SOURCE");
    println!("cargo:rerun-if-changed={}", manifest_path.display());
    dir
}
