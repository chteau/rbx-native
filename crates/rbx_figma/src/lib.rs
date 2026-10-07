//! Figma → Roblox GUI: sign-in, the REST API, and the translation behind the
//! UI Editor's "Import from Figma…".

pub mod api;
pub mod browse;
mod dotenv;
pub mod import;
pub mod infer;
pub mod link;
pub mod oauth;

/// A token to use instead of signing in, for development and automated
/// runs: `RBX_FIGMA_TOKEN` from the environment or, in debug builds only,
/// from the checkout's `.env`. A personal access token (`figd_…`) or an
/// OAuth access token; either never expires as far as this app knows, so it
/// is never refreshed or stored.
pub fn dev_token() -> Option<oauth::Tokens> {
    let token = std::env::var("RBX_FIGMA_TOKEN")
        .ok()
        .filter(|t| !t.trim().is_empty())
        .or_else(|| {
            if !cfg!(debug_assertions) {
                return None;
            }
            let env = dotenv::find(std::path::Path::new(env!("CARGO_MANIFEST_DIR")))?;
            dotenv::value_of(&std::fs::read_to_string(env).ok()?, "RBX_FIGMA_TOKEN")
        })?;
    Some(oauth::Tokens {
        access_token: token.trim().to_string(),
        refresh_token: String::new(),
        expires_at: u64::MAX,
    })
}
