//! Where the Figma window's answers come from, all blocking (run them on a
//! background thread): the REST API through a signed-in session, or a
//! fixture directory for captures (`RBX_STUDIO_FIGMA_FIXTURE`).
//!
//! Every call hands back the session's tokens when it refreshed them,
//! *beside* its result: an expired access token is gone once renewed, so the
//! new pair must reach the keyring even when the call itself then fails.
//!
//! A fixture directory holds `recent.json` (the home list, never written
//! back), `file.json` (a `GET /v1/files/:key` answer: the outline, and each
//! row's children), `node.json` (the frame the review step infers from),
//! `preview.png` (every row's preview) and the thumbnails `recent.json`
//! names by file name in `thumbnail_url`. Import there skips the uploads.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rbx_cloud::{ApiKey, Client};
use rbx_figma::api::Session;
use rbx_figma::browse::{self, Item, Outline, Recent, RecentFile};
use rbx_figma::import::{self, Cache};
use rbx_figma::infer::{self, Node};
use rbx_figma::link::Link;
use rbx_figma::oauth::Tokens;
use serde_json::Value;

pub(super) const FIXTURE_VARIABLE: &str = "RBX_STUDIO_FIGMA_FIXTURE";

const THUMB_AGE: Duration = Duration::from_secs(24 * 60 * 60);
const PREVIEW_AGE: Duration = Duration::from_secs(60 * 60);

/// A call's answer, and the tokens again if the session renewed them.
pub(super) type Reply<T> = (Result<T, String>, Option<Tokens>);

#[derive(Debug, Clone)]
pub(super) enum Source {
    Live(Tokens),
    Fixture(PathBuf),
}

fn figma_dir() -> Option<PathBuf> {
    rbx_assets::cache_root().map(|root| root.join("figma"))
}

/// Runs `call` in a session on `tokens`; see the module doc.
pub(super) fn in_session<T>(
    tokens: Tokens,
    call: impl FnOnce(&mut Session) -> Result<T, String>,
) -> Reply<T> {
    let mut session = Session::new(tokens);
    let result = call(&mut session);
    let refreshed = session.refreshed.then(|| session.tokens().clone());
    (result, refreshed)
}

/// The home list, signed in or not. A fixture's is read only, so opening a
/// file there doesn't rewrite the fixture.
pub(super) fn recent(source: Option<&Source>) -> Recent {
    match source {
        Some(Source::Fixture(dir)) => {
            let mut recent = Recent::default();
            recent.files = Recent::open(dir.join("recent.json")).files;
            recent
        }
        _ => figma_dir()
            .map(|dir| Recent::open(dir.join("recent.json")))
            .unwrap_or_default(),
    }
}

fn read_json(path: &Path) -> Result<Value, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// The node with `id` anywhere in a file's JSON.
fn find_json<'a>(v: &'a Value, id: &str) -> Option<&'a Value> {
    if v.get("id").and_then(Value::as_str) == Some(id) {
        return Some(v);
    }
    v.get("children")?
        .as_array()?
        .iter()
        .find_map(|c| find_json(c, id))
}

impl Source {
    pub fn fixture() -> Option<Source> {
        std::env::var_os(FIXTURE_VARIABLE).map(|dir| Source::Fixture(dir.into()))
    }

    fn run<T>(
        self,
        live: impl FnOnce(&mut Session) -> Result<T, String>,
        fixture: impl FnOnce(&Path) -> Result<T, String>,
    ) -> Reply<T> {
        match self {
            Source::Live(tokens) => in_session(tokens, live),
            Source::Fixture(dir) => (fixture(&dir), None),
        }
    }

    /// Each recent file's thumbnail on disk, by file key; files whose
    /// thumbnail couldn't be had are left out.
    pub fn thumbnails(self, files: Vec<RecentFile>) -> Reply<Vec<(String, PathBuf)>> {
        let wanted = files
            .into_iter()
            .filter_map(|f| Some((f.key, f.thumbnail_url?)))
            .collect::<Vec<_>>();
        let fixture_wanted = wanted.clone();
        self.run(
            move |session| {
                let dir = figma_dir().ok_or("No cache directory")?.join("thumbs");
                Ok(wanted
                    .into_iter()
                    .filter_map(|(key, url)| {
                        let path =
                            browse::cached_image(&dir, &key, THUMB_AGE, || session.download(&url))
                                .ok()?;
                        Some((key, path))
                    })
                    .collect())
            },
            move |dir| {
                Ok(fixture_wanted
                    .into_iter()
                    .map(|(key, name)| (key, dir.join(name)))
                    .collect())
            },
        )
    }

    pub fn outline(self, key: String) -> Reply<Outline> {
        let fixture_key = key.clone();
        self.run(
            move |session| browse::outline(session, &key),
            move |dir| {
                Ok(browse::outline_of(
                    &fixture_key,
                    &read_json(&dir.join("file.json"))?,
                ))
            },
        )
    }

    pub fn children(self, key: String, id: String) -> Reply<Vec<Item>> {
        let fixture_id = id.clone();
        self.run(
            move |session| browse::children(session, &key, &id),
            move |dir| {
                let file = read_json(&dir.join("file.json"))?;
                let node = file
                    .get("document")
                    .and_then(|d| find_json(d, &fixture_id))
                    .ok_or_else(|| format!("No node {fixture_id} in the fixture"))?;
                Ok(browse::item_of(node, 1).children.unwrap_or_default())
            },
        )
    }

    /// A small render of node `id`, cached on disk for an hour.
    pub fn preview(self, key: String, id: String) -> Reply<PathBuf> {
        self.run(
            move |session| {
                let dir = figma_dir().ok_or("No cache directory")?.join("previews");
                browse::cached_image(&dir, &format!("{key}/{id}"), PREVIEW_AGE, || {
                    let urls = browse::preview_urls(session, &key, std::slice::from_ref(&id))?;
                    let url = urls.get(&id).ok_or("Figma has no picture of that node")?;
                    session.download(url)
                })
            },
            |dir| Ok(dir.join("preview.png")),
        )
    }

    /// Figma's own render of the frame at 1x, saved in `dir` as
    /// `<id>.figma.png` (`RBX_STUDIO_FIGMA_REFERENCE`).
    pub fn reference(self, link: Link, dir: PathBuf) -> Reply<PathBuf> {
        self.run(
            move |session| {
                let ids = url::form_urlencoded::byte_serialize(link.node_id.as_bytes())
                    .collect::<String>();
                let answer = session.get_json(&format!(
                    "/v1/images/{}?ids={ids}&format=png&scale=1",
                    link.file_key
                ))?;
                let url = answer
                    .get("images")
                    .and_then(|images| images.get(&link.node_id))
                    .and_then(Value::as_str)
                    .ok_or("Figma has no picture of that node")?;
                let png = session.download(url)?;
                let path = dir.join(format!("{}.figma.png", link.node_id.replace(':', "-")));
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                std::fs::write(&path, png).map_err(|e| e.to_string())?;
                Ok(path)
            },
            |_| Err("No reference render offline".into()),
        )
    }

    /// Reads the frame and infers its tree; nothing is uploaded yet.
    pub fn prepare(self, link: Link, progress: Arc<Mutex<String>>) -> Reply<Node> {
        let dump = figma_dir().map(|dir| dir.join("last-import"));
        self.run(
            |session| {
                import::prepare(
                    session,
                    &link,
                    |line| *progress.lock().unwrap() = line,
                    dump.as_deref(),
                )
            },
            |dir| infer::infer(&read_json(&dir.join("node.json"))?),
        )
    }

    /// Renders and uploads the reviewed tree's pictures through the Open
    /// Cloud key.
    pub fn finish(self, file_key: String, tree: Node, progress: Arc<Mutex<String>>) -> Reply<Node> {
        let Source::Live(tokens) = self else {
            return (Ok(tree), None);
        };
        let describe = crate::shell::freeze::describe;
        let key =
            match ApiKey::from_env_or_config() {
                Some(key) => key,
                None => return (
                    Err(
                        "No Open Cloud API key is set up. Add one from Home \u{203a} Manage key."
                            .into(),
                    ),
                    None,
                ),
            };
        let client = Client::new(Some(key));
        let user = match client.introspect() {
            Ok(answer) => answer.authorized_user_id,
            Err(e) => return (Err(describe(&e)), None),
        };
        let dir = figma_dir();
        let dump = dir.as_ref().map(|dir| dir.join("last-import"));
        let mut cache = dir
            .map(|dir| Cache::open(dir.join(format!("uploads-{user}.json"))))
            .unwrap_or_default();
        in_session(tokens, |session| {
            import::finish(
                session,
                &file_key,
                tree,
                &mut cache,
                |name, png| {
                    client
                        .create_image_asset(name, "Imported from Figma by rbx-native.", user, png)
                        .map_err(|e| describe(&e))
                },
                |line| *progress.lock().unwrap() = line,
                dump.as_deref(),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{find_json, recent, Source};
    use std::path::PathBuf;

    #[test]
    fn a_fixture_answers_from_disk_without_tokens() {
        let dir: PathBuf = [env!("CARGO_MANIFEST_DIR"), "tests", "fixtures", "figma"]
            .iter()
            .collect();
        let source = Source::Fixture(dir);
        let (outline, refreshed) = source.clone().outline("KEY".into());
        assert!(refreshed.is_none());
        let outline = outline.unwrap();
        assert!(!outline.pages.is_empty());
        assert!(!recent(Some(&source)).files.is_empty());
        let (tree, _) = source.prepare(
            rbx_figma::link::Link {
                file_key: "KEY".into(),
                node_id: "1:1".into(),
            },
            Default::default(),
        );
        assert!(tree.unwrap().is_design());
    }

    #[test]
    fn nodes_are_found_at_any_depth() {
        let file = serde_json::json!({ "id": "0:0", "children": [
            { "id": "1:1", "children": [{ "id": "2:2" }] }
        ]});
        assert!(find_json(&file, "2:2").is_some());
        assert!(find_json(&file, "3:3").is_none());
    }
}
