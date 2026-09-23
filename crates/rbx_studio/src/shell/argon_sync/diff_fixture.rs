//! `RBX_STUDIO_ARGON_DIFF=1`'s batch: the review the Diff window's own
//! reference renders were drawn from, seeded on top of whatever place is
//! open. Twelve additions, three updates and one removal, with their
//! parents, the updated instances and the removed script put into the
//! DOM first so every "before" reads back the way a live review's would.
//!
//! The window's own state for a capture comes from two more variables it
//! reads itself: `RBX_STUDIO_ARGON_DIFF_SELECT=<name>` picks the selected
//! change and `RBX_STUDIO_ARGON_DIFF_COLLAPSE=additions,updates,removals`
//! starts those sections collapsed, `RBX_STUDIO_ARGON_DIFF_EXPAND=Lobby,
//! Portal` starts those nested additions open.

use rbx_dom::{BrickColor, Color3Data, Ref, Variant, Vector3Data};

use crate::argon_client::{self, ArgonRef, Snapshot, UpdatedSnapshot};
use crate::script_editor::source;

use super::diff_rows::bind;
use super::Shell;

const ROUND_CONTROLLER_OLD: &str =
    include_str!("../../../../../assets/tests/argon_diff/RoundController.old.luau");
const ROUND_CONTROLLER_NEW: &str =
    include_str!("../../../../../assets/tests/argon_diff/RoundController.new.luau");
const OLD_SHOP: &str = include_str!("../../../../../assets/tests/argon_diff/OldShop.luau");

impl Shell {
    /// Builds the batch and puts it up for review.
    pub(super) fn seed_debug_diff_pending(&mut self) {
        // Parents the additions land under; each gets an id so the rows can
        // name the path.
        let workspace = self.fixture_path(&["Workspace"]);
        let shared = self.fixture_path(&["ReplicatedStorage", "Shared"]);
        let packages = self.fixture_path(&["ReplicatedStorage", "Packages"]);
        let systems = self.fixture_path(&["ServerScriptService", "Systems"]);
        let starter_gui = self.fixture_path(&["StarterGui"]);
        let player_scripts = self.fixture_path(&["StarterPlayer", "StarterPlayerScripts"]);
        let replicated = self.fixture_path(&["ReplicatedStorage"]);
        let sound_service = self.fixture_path(&["SoundService"]);
        let server_storage = self.fixture_path(&["ServerStorage"]);
        let lighting = self.fixture_path(&["Lighting"]);

        let additions = vec![
            lobby(workspace),
            script("RoundConfig", "ModuleScript", shared, 42),
            script("Signal", "ModuleScript", packages, 118),
            script("Maid", "ModuleScript", packages, 74),
            script("VoteController", "Script", systems, 96),
            shop_ui(starter_gui),
            script("ShopClient", "LocalScript", player_scripts, 63),
            node(
                "Remotes",
                "Folder",
                replicated,
                vec![],
                (1..=6)
                    .map(|n| node(&format!("Remote{n}"), "RemoteEvent", None, vec![], vec![]))
                    .collect(),
            ),
            node(
                "CoinSound",
                "Sound",
                sound_service,
                vec![
                    ("SoundId", Variant::String("rbxassetid://9125644905".into())),
                    ("Volume", Variant::Float32(0.6)),
                    ("Looped", Variant::Bool(false)),
                ],
                vec![],
            ),
            leaderboard(workspace),
            script("DataKeys", "ModuleScript", server_storage, 18),
            node(
                "Fog",
                "Atmosphere",
                lighting,
                vec![
                    ("Density", Variant::Float32(0.35)),
                    ("Offset", Variant::Float32(0.2)),
                    ("Color", Variant::Color3(color(199, 199, 199))),
                    ("Decay", Variant::Color3(color(106, 112, 125))),
                    ("Glare", Variant::Float32(0.)),
                ],
                vec![],
            ),
        ];

        // Updates: the DOM holds the before state, the batch the after.
        let systems_ref = self.argon.ids[&systems.expect("Systems was just made")];
        let round_controller = self.fixture_child(systems_ref, "RoundController", "Script");
        source::write(&mut self.dom, round_controller, ROUND_CONTROLLER_OLD);
        let workspace_ref = self.argon.ids[&workspace.expect("Workspace was just made")];
        let spawn = self.fixture_child(workspace_ref, "SpawnLocation", "SpawnLocation");
        let grey = BrickColor::from_name("Medium stone grey").map_or(194, |c| c.number);
        let blue = BrickColor::from_name("Bright blue").map_or(23, |c| c.number);
        for (name, value) in [
            ("Color", Variant::Color3(color(163, 162, 165))),
            ("Duration", Variant::Float32(10.)),
            ("Neutral", Variant::Bool(true)),
            (
                "Size",
                Variant::Vector3(Vector3Data {
                    x: 12.,
                    y: 1.,
                    z: 12.,
                }),
            ),
            ("TeamColor", Variant::BrickColor(grey)),
        ] {
            let _ = self.dom.set_property(spawn, name, value);
        }
        let lighting_ref = self.argon.ids[&lighting.expect("Lighting was just made")];
        for (name, value) in [
            ("Brightness", Variant::Float32(2.)),
            ("ClockTime", Variant::Float32(14.)),
        ] {
            let _ = self.dom.set_property(lighting_ref, name, value);
        }
        let updates = vec![
            UpdatedSnapshot {
                id: bind(self, round_controller),
                name: None,
                class: None,
                properties: Some(encoded(vec![(
                    "Source",
                    Variant::String(ROUND_CONTROLLER_NEW.to_owned()),
                )])),
            },
            UpdatedSnapshot {
                id: bind(self, spawn),
                name: None,
                class: None,
                properties: Some(encoded(vec![
                    ("Color", Variant::Color3(color(108, 127, 219))),
                    ("Duration", Variant::Float32(0.)),
                    ("Neutral", Variant::Bool(false)),
                    (
                        "Size",
                        Variant::Vector3(Vector3Data {
                            x: 16.,
                            y: 1.,
                            z: 16.,
                        }),
                    ),
                    ("TeamColor", Variant::BrickColor(blue)),
                ])),
            },
            UpdatedSnapshot {
                id: bind(self, lighting_ref),
                name: None,
                class: None,
                properties: Some(encoded(vec![
                    ("Brightness", Variant::Float32(3.)),
                    ("ClockTime", Variant::Float32(18.)),
                ])),
            },
        ];

        let scripts_ref =
            self.argon.ids[&player_scripts.expect("StarterPlayerScripts was just made")];
        let old_shop = self.fixture_child(scripts_ref, "OldShop", "LocalScript");
        source::write(&mut self.dom, old_shop, OLD_SHOP);
        let removals = vec![bind(self, old_shop)];

        self.set_pending(argon_client::Changes {
            additions,
            updates,
            removals,
        });
    }

    /// The instance at `names` from the roots, made if missing (a root
    /// service by its class name, a `Folder` below), and its Argon id.
    fn fixture_path(&mut self, names: &[&str]) -> Option<ArgonRef> {
        let mut current: Option<Ref> = None;
        for (depth, name) in names.iter().enumerate() {
            let class = if depth == 0 { name } else { "Folder" };
            current = Some(match current {
                None => {
                    let roots = self.dom.root_refs().to_vec();
                    roots
                        .into_iter()
                        .find(|&r| self.dom.get(r).is_some_and(|i| i.name() == *name))
                        .unwrap_or_else(|| self.dom.new_instance(class, name, None))
                }
                Some(parent) => self.fixture_child(parent, name, class),
            });
        }
        current.map(|referent| bind(self, referent))
    }

    fn fixture_child(&mut self, parent: Ref, name: &str, class: &str) -> Ref {
        let existing = self.dom.get(parent).and_then(|instance| {
            instance
                .children()
                .iter()
                .copied()
                .find(|&child| self.dom.get(child).is_some_and(|c| c.name() == name))
        });
        existing.unwrap_or_else(|| self.dom.new_instance(class, name, Some(parent)))
    }
}

fn color(r: u8, g: u8, b: u8) -> Color3Data {
    Color3Data {
        r: f32::from(r) / 255.,
        g: f32::from(g) / 255.,
        b: f32::from(b) / 255.,
    }
}

fn encoded(properties: Vec<(&str, Variant)>) -> Vec<(String, rmpv::Value)> {
    properties
        .into_iter()
        .filter_map(|(name, value)| Some((name.to_owned(), argon_client::encode_value(&value)?)))
        .collect()
}

fn node(
    name: &str,
    class: &str,
    parent: Option<ArgonRef>,
    properties: Vec<(&str, Variant)>,
    children: Vec<Snapshot>,
) -> Snapshot {
    Snapshot {
        id: ArgonRef::generate(),
        parent,
        name: name.to_owned(),
        class: class.to_owned(),
        properties: encoded(properties),
        children,
        keep_unknowns: false,
    }
}

/// A script of exactly `lines` lines of filler.
fn script(name: &str, class: &str, parent: Option<ArgonRef>, lines: usize) -> Snapshot {
    let body: Vec<String> = (1..=lines)
        .map(|n| match n {
            1 => format!("-- {name}"),
            n if n == lines => {
                format!("return {name}").replace("return VoteController", "return nil")
            }
            n => format!("local line{n} = {n}"),
        })
        .collect();
    node(
        name,
        class,
        parent,
        vec![("Source", Variant::String(body.join("\n")))],
        vec![],
    )
}

fn parts(prefix: &str, count: usize) -> Vec<Snapshot> {
    (1..=count)
        .map(|n| node(&format!("{prefix}{n}"), "Part", None, vec![], vec![]))
        .collect()
}

/// Lobby: 38 nested — Part 22, Folder 4, Model 3, Script 3, PointLight 4,
/// Sound 2.
fn lobby(parent: Option<ArgonRef>) -> Snapshot {
    let mut spawns_children = parts("SpawnPad", 8);
    spawns_children.push(node("Markers", "Folder", None, vec![], parts("Marker", 2)));
    spawns_children.push(script("SpawnManager", "Script", None, 12));
    let mut decor_children = parts("Pillar", 10);
    decor_children.push(node(
        "Lamps",
        "Folder",
        None,
        vec![],
        (1..=3)
            .map(|n| node(&format!("Lamp{n}"), "PointLight", None, vec![], vec![]))
            .collect(),
    ));
    decor_children.push(node("Bench", "Model", None, vec![], vec![]));
    decor_children.push(node("Statue", "Model", None, vec![], vec![]));
    decor_children.push(node("Ambience", "Sound", None, vec![], vec![]));
    decor_children.push(script("DecorSpin", "Script", None, 9));
    node(
        "Lobby",
        "Model",
        parent,
        vec![
            ("LevelOfDetail", Variant::Enum(0)),
            ("ModelStreamingMode", Variant::Enum(0)),
        ],
        vec![
            node("Floor", "Part", None, vec![], vec![]),
            node("Spawns", "Folder", None, vec![], spawns_children),
            node(
                "Portal",
                "Model",
                None,
                vec![],
                vec![
                    node("Frame", "Part", None, vec![], vec![]),
                    node("Glow", "PointLight", None, vec![], vec![]),
                    script("PortalTouch", "Script", None, 42),
                ],
            ),
            node("Decor", "Folder", None, vec![], decor_children),
            node("LobbyMusic", "Sound", None, vec![], vec![]),
        ],
    )
}

/// ShopUI: 21 nested.
fn shop_ui(parent: Option<ArgonRef>) -> Snapshot {
    let mut main = Vec::new();
    for n in 1..=5 {
        main.push(node(&format!("Buy{n}"), "TextButton", None, vec![], vec![]));
        main.push(node(
            &format!("Label{n}"),
            "TextLabel",
            None,
            vec![],
            vec![],
        ));
    }
    for n in 1..=3 {
        main.push(node(
            &format!("Icon{n}"),
            "ImageLabel",
            None,
            vec![],
            vec![],
        ));
    }
    main.push(node("Rows", "UIListLayout", None, vec![], vec![]));
    main.push(node("Columns", "UIListLayout", None, vec![], vec![]));
    main.push(node("Corner", "UICorner", None, vec![], vec![]));
    node(
        "ShopUI",
        "ScreenGui",
        parent,
        vec![],
        vec![
            node("Main", "Frame", None, vec![], main),
            node("Header", "Frame", None, vec![], vec![]),
            node("Footer", "Frame", None, vec![], vec![]),
            node("CloseButton", "TextButton", None, vec![], vec![]),
            node("Overlay", "Frame", None, vec![], vec![]),
        ],
    )
}

/// Leaderboard: 14 nested.
fn leaderboard(parent: Option<ArgonRef>) -> Snapshot {
    node(
        "Leaderboard",
        "Model",
        parent,
        vec![],
        vec![
            node("Board", "Part", None, vec![], vec![]),
            node(
                "Surface",
                "SurfaceGui",
                None,
                vec![],
                (1..=12)
                    .map(|n| node(&format!("Row{n}"), "TextLabel", None, vec![], vec![]))
                    .collect(),
            ),
        ],
    )
}
