//! The instance-tree half of Studio's Save to Roblox check: which instances
//! an accessory may hold, tags, attributes and the few properties it reads.
//! Studio publishes no page for these; every rule and message here was
//! matched against Studio 0.745 on the same items (see the UGC creators
//! research notes). Studio stops at the first schema failure; this reports
//! everything at once.

use crate::spec::{AssetType, Category};
use crate::{Finding, Rule, Severity};

/// One instance of the item, as much of it as the rules read.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Node {
    pub class: String,
    pub name: String,
    pub tagged: bool,
    pub attributed: bool,
    /// A `StringValue`'s `Value`.
    pub value: Option<String>,
    /// A part's `Anchored`.
    pub anchored: bool,
    /// A `MeshPart` whose `Size` is not its `MeshSize`.
    pub scaled: bool,
    /// A `WrapLayer` with a `ReferenceMeshId`.
    pub reference_mesh: bool,
    /// A `WrapLayer`'s `AutoSkin` is `EnabledOverride`.
    pub auto_skin_override: bool,
    pub children: Vec<Node>,
}

impl Node {
    fn child(&self, class: &str, name: &str) -> Option<&Node> {
        self.children
            .iter()
            .find(|c| c.class == class && c.name == name)
    }
}

const SCALE_TYPES: [&str; 3] = ["Classic", "ProportionsSlender", "ProportionsNormal"];

fn finding(rule: Rule, subject: &str, message: String) -> Finding {
    Finding {
        rule,
        severity: Severity::Error,
        subject: subject.to_string(),
        measured: None,
        limit: None,
        message,
    }
}

/// Every path under `node` (itself included), parent first.
fn all_paths(node: &Node, path: &str, out: &mut Vec<String>) {
    out.push(path.to_string());
    for c in &node.children {
        all_paths(c, &format!("{path}.{}", c.name), out);
    }
}

fn walk(node: &Node, path: &str, f: &mut impl FnMut(&Node, &str)) {
    f(node, path);
    for c in &node.children {
        walk(c, &format!("{path}.{}", c.name), f);
    }
}

/// Findings for an `Accessory` tree checked as `asset`. The missing-handle
/// case is the caller's: without a handle there is nothing else to check.
pub fn check(root: &Node, asset: AssetType) -> Vec<Finding> {
    let mut out = Vec::new();
    let item = root.name.as_str();
    let layered = asset.category() == Category::Layered;
    let mut stray = Vec::new();
    let mut handle_node: Option<&Node> = None;

    for c in &root.children {
        let path = format!("{item}.{}", c.name);
        let is_handle = c.name == "Handle" && matches!(c.class.as_str(), "MeshPart" | "Part");
        if is_handle && handle_node.is_none() {
            handle_node = Some(c);
            handle(c, item, &path, asset, layered, &mut stray, &mut out);
        } else if c.class == "Configuration" && c.name == "ThumbnailConfiguration" {
            for (class, name) in [
                ("ObjectValue", "ThumbnailCameraTarget"),
                ("CFrameValue", "ThumbnailCameraValue"),
            ] {
                if c.child(class, name).is_none() {
                    out.push(finding(
                        Rule::Schema,
                        &path,
                        format!("Could not find required {class} called {name} inside of {path}"),
                    ));
                }
            }
            for k in &c.children {
                let known = matches!(
                    (k.class.as_str(), k.name.as_str()),
                    ("ObjectValue", "ThumbnailCameraTarget")
                        | ("CFrameValue", "ThumbnailCameraValue")
                );
                if !known || !k.children.is_empty() {
                    let at = format!("{path}.{}", k.name);
                    if known {
                        for g in &k.children {
                            all_paths(g, &format!("{at}.{}", g.name), &mut stray);
                        }
                    } else {
                        all_paths(k, &at, &mut stray);
                    }
                }
            }
        } else {
            all_paths(c, &path, &mut stray);
        }
    }

    if !stray.is_empty() {
        out.push(finding(
            Rule::Schema,
            item,
            format!(
                "The following instances are not part of the approved schema and should be removed: {}.",
                stray.join(", ")
            ),
        ));
    }

    let mut tagged = Vec::new();
    let mut attributed = Vec::new();
    walk(root, item, &mut |n, path| {
        if n.tagged {
            tagged.push(path.to_string());
        }
        // Studio lets the Handle itself keep attributes.
        if n.attributed && !handle_node.is_some_and(|h| std::ptr::eq(h, n)) {
            attributed.push(path.to_string());
        }
    });
    if !tagged.is_empty() {
        out.push(finding(
            Rule::Tags,
            item,
            format!(
                "The following instances have tags but marketplace assets cannot have any tags: {}",
                tagged.join(", ")
            ),
        ));
    }
    if !attributed.is_empty() {
        out.push(finding(
            Rule::Attributes,
            item,
            format!(
                "'{item}' (or its descendants) contain Attributes. You need to remove Attributes from the following: {}",
                attributed.join(", ")
            ),
        ));
    }
    out
}

fn handle(
    h: &Node,
    item: &str,
    path: &str,
    asset: AssetType,
    layered: bool,
    stray: &mut Vec<String>,
    out: &mut Vec<Finding>,
) {
    let allowed = asset.attachments();
    let mut attachment_seen = false;
    let mut wrap_seen = false;
    for c in &h.children {
        let at = format!("{path}.{}", c.name);
        let fits = match (c.class.as_str(), c.name.as_str()) {
            ("Attachment", name) if !attachment_seen && allowed.contains(&name) => {
                attachment_seen = true;
                // Roblox's own items keep the position the fitting tool
                // started from here.
                for k in &c.children {
                    if !(k.class == "Vector3Value" && k.name == "OriginalPosition") {
                        all_paths(k, &format!("{at}.{}", k.name), stray);
                    }
                }
                continue;
            }
            ("StringValue", "AvatarPartScaleType") => {
                if !c.value.as_deref().is_some_and(|v| SCALE_TYPES.contains(&v)) {
                    out.push(finding(
                        Rule::Schema,
                        &at,
                        "The AvatarPartScaleType value in 'Handle' is invalid. Accepted values are Classic, ProportionsSlender, or ProportionsNormal.".to_string(),
                    ));
                }
                true
            }
            ("Vector3Value", "OriginalSize") | ("SurfaceAppearance", _) => true,
            ("SpecialMesh", _) => h.class == "Part",
            ("WrapLayer", _) if layered && !wrap_seen => {
                wrap_seen = true;
                wrap(c, item, &at, asset, out);
                true
            }
            ("WrapLayer", _) if !layered => {
                out.push(finding(
                    Rule::Schema,
                    &at,
                    format!(
                        "Asset type '{}' is not a layered clothing category. It can only be used with rigid accessories.",
                        asset.type_name()
                    ),
                ));
                false
            }
            _ => false,
        };
        if fits {
            for k in &c.children {
                all_paths(k, &format!("{at}.{}", k.name), stray);
            }
        } else {
            all_paths(c, &at, stray);
        }
    }
    if h.anchored {
        out.push(finding(
            Rule::Property,
            path,
            format!("Property 'Anchored' on '{path}' is invalid. Expected to be = false."),
        ));
    }
    if layered && h.scaled {
        out.push(finding(
            Rule::Property,
            path,
            "Handle has been scaled, but mesh parts with wrap layers do not support scaling. You need to change the Size property to match the MeshSize property.".to_string(),
        ));
    }
}

fn wrap(w: &Node, item: &str, path: &str, asset: AssetType, out: &mut Vec<Finding>) {
    if !w.reference_mesh {
        out.push(finding(
            Rule::Hsr,
            path,
            format!(
                "Missing InnerCage (i.e. invalid ReferenceMeshId) on layered clothing accessory '{item}'. Make sure you are using a valid meshId and try again."
            ),
        ));
        out.push(finding(
            Rule::Hsr,
            path,
            format!("Missing HSR data for '{path}'. Please try again."),
        ));
    }
    if matches!(asset, AssetType::Eyebrow | AssetType::Eyelash) && !w.auto_skin_override {
        out.push(finding(
            Rule::Skinning,
            path,
            format!(
                "{} must use skinning transfer, please set WrapLayer.AutoSkin to EnabledOverride.",
                asset.type_name()
            ),
        ));
    }
}

/// Studio reads a shoe bundle as a `Model` holding both shoes under these
/// names, and refuses anything else ("Expected LeftShoeAccessory and
/// RightShoeAccessory for shoe validation", in its log; its dialog only says
/// "Unknown error").
pub fn shoe_pair(model: &Node) -> Option<Finding> {
    let has = |name: &str| {
        model
            .children
            .iter()
            .any(|c| c.class == "Accessory" && c.name == name)
    };
    if has("LeftShoeAccessory") && has("RightShoeAccessory") {
        return None;
    }
    Some(finding(
        Rule::Schema,
        &model.name,
        "Expected LeftShoeAccessory and RightShoeAccessory for shoe validation. Put both shoes in one Model, named LeftShoeAccessory and RightShoeAccessory.".to_string(),
    ))
}

#[cfg(test)]
#[path = "schema/tests.rs"]
mod tests;
