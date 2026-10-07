//! A Figma link as the share menu copies it:
//! `https://www.figma.com/design/<key>/<title>?node-id=1-2`.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub file_key: String,
    /// In the API's spelling, `1:2`.
    pub node_id: String,
}

pub fn parse(text: &str) -> Result<Link, String> {
    let url = url::Url::parse(text.trim()).map_err(|_| "That isn\u{2019}t a link".to_string())?;
    let host = url.host_str().unwrap_or("");
    if host != "figma.com" && !host.ends_with(".figma.com") {
        return Err("That isn\u{2019}t a figma.com link".into());
    }
    let mut segments = url.path_segments().into_iter().flatten();
    let file_key = match (segments.next(), segments.next()) {
        (Some("design" | "file" | "proto" | "board"), Some(key))
            if !key.is_empty() && key.bytes().all(|b| b.is_ascii_alphanumeric()) =>
        {
            key.to_string()
        }
        _ => return Err("That Figma link names no file".into()),
    };
    let node_id = url
        .query_pairs()
        .find(|(key, _)| key == "node-id")
        .map(|(_, value)| value.replace('-', ":"))
        .filter(|id| !id.is_empty())
        .ok_or("Select a frame in Figma and copy its link (Copy link to selection): this one names no frame")?;
    Ok(Link { file_key, node_id })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn design_and_file_links_parse() {
        let link = parse("https://www.figma.com/design/AbC123/My-UI?node-id=1-2&t=x").unwrap();
        assert_eq!(
            link,
            Link {
                file_key: "AbC123".into(),
                node_id: "1:2".into()
            }
        );
        let old = parse(" https://figma.com/file/K9/x?node-id=10%3A20 ").unwrap();
        assert_eq!(old.node_id, "10:20");
        assert!(parse("https://www.figma.com/design/AbC123/My-UI")
            .unwrap_err()
            .contains("names no frame"));
        assert!(parse("https://evil.example/design/A/B?node-id=1-2").is_err());
        assert!(parse("https://www.figma.com/community/x?node-id=1-2").is_err());
    }
}
