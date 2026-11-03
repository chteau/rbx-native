//! Trusting a download: `SHA256SUMS` must carry a minisign signature from
//! the release key below, and the package must hash to its line there.

use minisign_verify::{PublicKey, Signature};

/// The release signing key (minisign key id 4A730FA919B3E7C7). Its secret
/// half is the `MINISIGN_SECRET_KEY` secret the release workflow signs with.
pub(super) const PUBLIC_KEY: &str = "RWTH57MZqQ9zSpsJv14IQQqWjzpg9EdLyKUQ610z7gC7zShLMJb5kuJ+";

/// Checks `minisig` (a `.minisig` file's text) over `data`.
pub(super) fn signature(public_key: &str, data: &[u8], minisig: &str) -> Result<(), String> {
    let key =
        PublicKey::from_base64(public_key).map_err(|error| format!("bad public key: {error}"))?;
    let signature =
        Signature::decode(minisig).map_err(|error| format!("unreadable signature: {error}"))?;
    key.verify(data, &signature, false)
        .map_err(|_| "SHA256SUMS is not signed by the RbxNative release key".to_owned())
}

/// The lowercase hex SHA-256 `sums` (`sha256sum` output) lists for `name`.
pub(super) fn sum_for(sums: &str, name: &str) -> Result<String, String> {
    sums.lines()
        .find_map(|line| {
            let (hash, file) = line.split_once(char::is_whitespace)?;
            let file = file.trim_start().trim_start_matches('*');
            (file == name).then(|| hash.to_ascii_lowercase())
        })
        .filter(|hash| hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| format!("SHA256SUMS lists no checksum for {name}"))
}

pub(super) fn hex(digest: &[u8]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}
