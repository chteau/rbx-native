//! Parsing of the asset reference strings found in `.rbxl`/`.rbxm` property
//! values (`Content` properties, texture ids, sound ids, ...).

use crate::error::AssetRefError;

/// A parsed asset reference, as it would appear in a `Content` property.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AssetRef {
    /// `rbxassetid://<id>` or a legacy `roblox.com`/`assetdelivery.roblox.com`
    /// URL carrying an `id=` query parameter.
    Id(u64),
    /// `rbxasset://<path>`: content shipped with Roblox Studio itself, not
    /// hosted per-asset. `path` is the relative path with no leading slash,
    /// e.g. `"textures/SpawnLocation.png"`.
    Native(String),
    /// `rbxthumb://...`: a thumbnail reference. Kept opaque (the part after
    /// the scheme) since this crate does not resolve thumbnails.
    Thumb(String),
    /// The empty string, used by Roblox to mean "no asset".
    Empty,
}

impl AssetRef {
    /// Parses any of the reference forms Roblox embeds in place files.
    pub fn parse(input: &str) -> Result<AssetRef, AssetRefError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(AssetRef::Empty);
        }

        let Some(scheme_end) = trimmed.find("://") else {
            return Err(AssetRefError::UnknownScheme(trimmed.to_string()));
        };
        let scheme = trimmed[..scheme_end].trim();
        let rest = trimmed[scheme_end + 3..].trim();

        match scheme.to_ascii_lowercase().as_str() {
            "rbxassetid" => parse_numeric_id(rest, trimmed),
            "rbxasset" => Ok(AssetRef::Native(rest.to_string())),
            "rbxthumb" => Ok(AssetRef::Thumb(rest.to_string())),
            "http" | "https" => parse_legacy_url(trimmed),
            _ => Err(AssetRefError::UnknownScheme(trimmed.to_string())),
        }
    }
}

fn parse_numeric_id(rest: &str, original: &str) -> Result<AssetRef, AssetRefError> {
    if rest.is_empty() {
        return Err(AssetRefError::MissingId(original.to_string()));
    }
    rest.parse::<u64>()
        .map(AssetRef::Id)
        .map_err(|_| AssetRefError::InvalidId(original.to_string()))
}

/// Legacy asset URLs only resolve on `roblox.com` hosts; anything else (a
/// third-party mirror, a typo) is treated as an unknown scheme rather than
/// silently trusted.
fn parse_legacy_url(original: &str) -> Result<AssetRef, AssetRefError> {
    let lower = original.to_ascii_lowercase();
    if !lower.contains("roblox.com") {
        return Err(AssetRefError::UnknownScheme(original.to_string()));
    }
    match extract_id_query_param(original) {
        Some(id) => Ok(AssetRef::Id(id)),
        None => Err(AssetRefError::MissingId(original.to_string())),
    }
}

/// Scans a query string for an `id=<digits>` parameter, requiring it to be
/// preceded by `?` or `&` so we don't match inside e.g. `userId=`.
fn extract_id_query_param(url: &str) -> Option<u64> {
    let lower = url.to_ascii_lowercase();
    let mut search_from = 0;
    while let Some(offset) = lower[search_from..].find("id=") {
        let pos = search_from + offset;
        let preceded_by_separator = pos == 0 || matches!(url.as_bytes()[pos - 1], b'?' | b'&');
        if preceded_by_separator {
            let digits: String = url[pos + 3..]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if let Ok(id) = digits.parse::<u64>() {
                return Some(id);
            }
        }
        search_from = pos + 3;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_string_is_empty() {
        assert_eq!(AssetRef::parse(""), Ok(AssetRef::Empty));
        assert_eq!(AssetRef::parse("   "), Ok(AssetRef::Empty));
    }

    #[test]
    fn parses_rbxassetid() {
        assert_eq!(
            AssetRef::parse("rbxassetid://6372755229"),
            Ok(AssetRef::Id(6372755229))
        );
    }

    #[test]
    fn scheme_is_case_insensitive() {
        assert_eq!(
            AssetRef::parse("RBXAssetId://6372755229"),
            Ok(AssetRef::Id(6372755229))
        );
    }

    #[test]
    fn tolerates_whitespace_after_scheme() {
        assert_eq!(
            AssetRef::parse("rbxassetid://  6372755229  "),
            Ok(AssetRef::Id(6372755229))
        );
    }

    #[test]
    fn parses_rbxasset_native_paths() {
        assert_eq!(
            AssetRef::parse("rbxasset://textures/SpawnLocation.png"),
            Ok(AssetRef::Native("textures/SpawnLocation.png".to_string()))
        );
        assert_eq!(
            AssetRef::parse("rbxasset://sky/sun.jpg"),
            Ok(AssetRef::Native("sky/sun.jpg".to_string()))
        );
    }

    #[test]
    fn parses_rbxthumb_opaquely() {
        assert_eq!(
            AssetRef::parse("rbxthumb://type=Asset&id=6372755229&w=150&h=150"),
            Ok(AssetRef::Thumb(
                "type=Asset&id=6372755229&w=150&h=150".to_string()
            ))
        );
    }

    #[test]
    fn parses_legacy_asset_url() {
        assert_eq!(
            AssetRef::parse("http://www.roblox.com/asset/?id=123"),
            Ok(AssetRef::Id(123))
        );
    }

    #[test]
    fn parses_legacy_double_slash_asset_url() {
        // Seen on a legacy UnionOperation.AssetId: Studio round-trips a stray
        // extra slash before "asset" that it never itself writes today.
        assert_eq!(
            AssetRef::parse("http://www.roblox.com//asset/?id=394314025"),
            Ok(AssetRef::Id(394314025))
        );
    }

    #[test]
    fn parses_legacy_assetdelivery_url() {
        assert_eq!(
            AssetRef::parse("https://assetdelivery.roblox.com/v1/asset?id=123"),
            Ok(AssetRef::Id(123))
        );
    }

    #[test]
    fn extracts_id_when_not_first_query_param() {
        assert_eq!(
            AssetRef::parse("https://assetdelivery.roblox.com/v1/asset?userId=1&id=456"),
            Ok(AssetRef::Id(456))
        );
    }

    #[test]
    fn does_not_match_id_suffix_inside_other_param() {
        // "userId=1" must not be mistaken for an "id=" parameter.
        let err = AssetRef::parse("https://assetdelivery.roblox.com/v1/asset?userId=1");
        assert!(matches!(err, Err(AssetRefError::MissingId(_))));
    }

    #[test]
    fn rejects_non_roblox_host() {
        let err = AssetRef::parse("http://example.com/asset?id=123");
        assert!(matches!(err, Err(AssetRefError::UnknownScheme(_))));
    }

    #[test]
    fn rejects_unknown_scheme() {
        let err = AssetRef::parse("foo://bar");
        assert!(matches!(err, Err(AssetRefError::UnknownScheme(_))));
    }

    #[test]
    fn rejects_string_without_scheme_separator() {
        let err = AssetRef::parse("not-a-uri");
        assert!(matches!(err, Err(AssetRefError::UnknownScheme(_))));
    }

    #[test]
    fn rejects_missing_id() {
        let err = AssetRef::parse("rbxassetid://");
        assert!(matches!(err, Err(AssetRefError::MissingId(_))));
    }

    #[test]
    fn rejects_non_numeric_id() {
        let err = AssetRef::parse("rbxassetid://not-a-number");
        assert!(matches!(err, Err(AssetRefError::InvalidId(_))));
    }
}
