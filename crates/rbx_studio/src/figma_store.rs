//! The Figma sign-in at rest: in the OS credential store next to the Open
//! Cloud key (see `key_store`), as the tokens' JSON — never a plaintext file.

use gpui_kit::AsyncApp;
use rbx_figma::oauth::Tokens;

/// The store's lookup key, apart from the Open Cloud key's.
const URL: &str = "https://api.figma.com/rbx-native";
const ACCOUNT: &str = "figma-oauth";

/// The stored sign-in, if there is one.
pub(crate) async fn load(cx: &mut AsyncApp) -> anyhow::Result<Option<Tokens>> {
    let stored = cx.update(|cx| cx.read_credentials(URL)).await?;
    Ok(stored.and_then(|(_, secret)| decode(&secret)))
}

/// Stores `tokens`, replacing any there.
pub(crate) async fn save(tokens: &Tokens, cx: &mut AsyncApp) -> anyhow::Result<()> {
    let secret = encode(tokens);
    cx.update(|cx| cx.write_credentials(URL, ACCOUNT, &secret)).await?;
    Ok(())
}

/// Signs out: the next import asks to connect again.
pub(crate) async fn forget(cx: &mut AsyncApp) -> anyhow::Result<()> {
    cx.update(|cx| cx.delete_credentials(URL)).await?;
    Ok(())
}

fn encode(tokens: &Tokens) -> Vec<u8> {
    serde_json::to_vec(tokens).expect("plain strings and a number")
}

/// `None` for anything unreadable: it means "connect again", not a crash.
fn decode(secret: &[u8]) -> Option<Tokens> {
    serde_json::from_slice(secret).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn figma_tokens_round_trip_through_the_stored_bytes() {
        let tokens = Tokens {
            access_token: "a".into(),
            refresh_token: "r".into(),
            expires_at: 42,
        };
        assert_eq!(decode(&encode(&tokens)), Some(tokens));
        assert_eq!(decode(b"not json"), None);
    }
}
