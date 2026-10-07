//! Bakes `RBX_FIGMA_CLIENT_SECRET` in from the nearest `.env` (gitignored)
//! above the crate when the build's own environment doesn't set it, so a
//! local build signs in without exporting the secret every time. A worktree
//! nested in the checkout finds the checkout's `.env`. The value is never
//! printed: only `cargo:rustc-env` carries it, into `option_env!`.
//!
//! A `.env` created after the last build isn't noticed until something
//! else reruns this script (`touch crates/rbx_figma/build.rs`): watching a
//! file that doesn't exist would rebuild the crate every time.

#[path = "src/dotenv.rs"]
#[allow(dead_code)]
mod dotenv;

const KEY: &str = "RBX_FIGMA_CLIENT_SECRET";

fn main() {
    println!("cargo:rerun-if-env-changed={KEY}");
    if std::env::var_os(KEY).is_some() {
        return;
    }
    let Some(manifest) = std::env::var_os("CARGO_MANIFEST_DIR") else {
        return;
    };
    let Some(env) = dotenv::find(std::path::Path::new(&manifest)) else {
        return;
    };
    println!("cargo:rerun-if-changed={}", env.display());
    let Ok(text) = std::fs::read_to_string(&env) else {
        return;
    };
    if let Some(value) = dotenv::value_of(&text, KEY) {
        println!("cargo:rustc-env={KEY}={value}");
    }
}
