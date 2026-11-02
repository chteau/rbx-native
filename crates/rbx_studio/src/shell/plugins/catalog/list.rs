//! The verified catalog: free Creator Store plugins that start and answer
//! their toolbar in rbx-native's host (the headless sweep in
//! `docs/plugins.md`). Only metadata lives here. The plugin itself is
//! downloaded from Roblox when the user installs it, never shipped.

/// One verified plugin. `name` and `creator` are shown until the store's
/// own listing arrives (and when it can't be reached); the blurb is ours.
pub(super) struct Verified {
    pub(super) id: u64,
    pub(super) name: &'static str,
    pub(super) creator: &'static str,
    pub(super) blurb: &'static str,
}

const fn v(id: u64, name: &'static str, creator: &'static str, blurb: &'static str) -> Verified {
    Verified {
        id,
        name,
        creator,
        blurb,
    }
}

/// Most voted first, as of the sweep; the live vote counts re-sort it.
pub(super) const VERIFIED: &[Verified] = &[
    v(
        752585459,
        "Load Character Lite",
        "AlreadyPro",
        "Drop any player's avatar into the place as a rig.",
    ),
    v(
        85716018250741,
        "Lemonade AI",
        "Lemonade Labs",
        "Lemonade's AI assistant for building games, in a panel.",
    ),
    v(
        144950355,
        "Building Tools by F3X",
        "GigsD4X",
        "Move, resize, rotate, paint and more from one toolset with handles.",
    ),
    v(
        144938633,
        "Archimedes",
        "Scriptos",
        "Builds smooth arcs and circles out of a selected part.",
    ),
    v(
        5070921519,
        "Resurface",
        "cxmeels",
        "Brings back the classic surface tool: studs, inlets, welds and hinges.",
    ),
    v(
        2273628561,
        "ThreeDText 2",
        "XAXA",
        "Turns typed text into 3D lettering made of mesh parts.",
    ),
    v(
        261634767,
        "Part to Terrain",
        "mkargus",
        "Paints terrain in the shape of the parts you click.",
    ),
    v(
        124244262358116,
        "Rebirth",
        "Ar_Link",
        "Connects Studio to the Rebirth web service.",
    ),
    v(
        165687726,
        "GapFill & Extrude",
        "stravant",
        "Fills the gap between two part edges, or extrudes a face.",
    ),
    v(
        1496745047,
        "AutoScale Lite",
        "ZacBytes",
        "Converts GUI sizes and positions between scale and offset.",
    ),
    v(
        1274343708,
        "RigEdit Lite",
        "Arch_Mage",
        "Edits a rig's joints and parts with a tree view.",
    ),
    v(
        400812710,
        "Realism Mod",
        "GizmoTjaz",
        "Adds head turning and footstep sounds to your characters.",
    ),
    v(
        2908380847,
        "Rig Editor",
        "goro7",
        "Creates, moves and scales the joints of custom rigs.",
    ),
    v(
        1602528167,
        "Asset Creator",
        "Stickmasterluke",
        "Turns a part placed on a character into an accessory or tool.",
    ),
    v(
        13964761992,
        "Lighting Shading Plugin",
        "Team MaikLP Studios",
        "One-click lighting and sky presets for the place.",
    ),
    v(
        2166774609,
        "Rain Plugin",
        "buildthomas",
        "Adds rain to the place, with its look and sound tuned in a panel.",
    ),
    v(
        165534573,
        "ResizeAlign",
        "stravant",
        "Resizes parts so the faces you pick meet exactly.",
    ),
    v(
        16708835782,
        "RBXMonkey",
        "CAUTlONED",
        "Moves animations between Blender and Roblox rigs.",
    ),
    v(
        2268520847,
        "Brushtool 2.1",
        "XAXA",
        "Brushes scattered copies of a part or model onto the map.",
    ),
    v(
        1921957669,
        "Avatar Importer",
        "Cytronyx",
        "Imports any user's avatar as an R6, R15 or Rthro rig.",
    ),
    v(
        14972318524,
        "Maor's Gui Animation",
        "123maor",
        "Keyframes GUI objects on a timeline.",
    ),
    v(
        716953901,
        "Blender Rig Exporter",
        "Den_S",
        "Exports a rig to Blender and brings the animation back.",
    ),
    v(
        1882309978,
        "Legacy Animation Editor",
        "buildthomas",
        "The older keyframe animation editor, kept as a plugin.",
    ),
    v(
        10546846471,
        "Moon Animator Export Cutscene",
        "rtvr56565",
        "Turns Moon Animator camera exports into parts for cutscenes.",
    ),
    v(
        15142310521,
        "Selection To Camera",
        "Dhammapada",
        "Brings the selection right in front of the camera.",
    ),
    v(
        7055415547,
        "Accessory Inserter",
        "KollowMC",
        "Inserts accessories, gear and clothing by id.",
    ),
    v(
        8867876284,
        "Load Roblox Characters 4",
        "Wolf1te",
        "Loads someone's avatar by user id or @name.",
    ),
    v(
        7229545707,
        "Infinite Terrain",
        "5uphi",
        "Generates and edits endless procedural terrain.",
    ),
    v(
        5828862351,
        "Surface Converter",
        "Useful_DoIphin",
        "Swaps the surface style of plastic parts.",
    ),
    v(
        260276858,
        "Studio Build Suite",
        "Fractality",
        "Precise move, rotate and resize by set increments.",
    ),
    v(
        1223999426,
        "Light Editor",
        "woot3",
        "Shows every light in the map and edits it in place.",
    ),
    v(
        883602387,
        "Smooth Cam",
        "sleitnick",
        "Eases the editor camera's movement for smoother fly-throughs.",
    ),
    v(
        3704545736,
        "Paint on Click",
        "chillthrill709",
        "Paints colour and material onto parts as you click them.",
    ),
    v(
        143152131,
        "Brick Cutter",
        "Ozzypig",
        "Cuts a part in two, or in four, along a dragged line.",
    ),
    v(
        10366079803,
        "Add Easy Texture",
        "RobocrafterLP",
        "Puts textures on part faces from a picker.",
    ),
    v(
        2233768483,
        "Roundify",
        "Stelrex",
        "Gives GUI frames rounded corners.",
    ),
    v(
        6274600541,
        "Camera Light",
        "sleitnick",
        "A light that follows the camera while you build in the dark.",
    ),
    v(
        143613186,
        "Model Resize Plugin 2.1",
        "DaMrNelson",
        "Scales whole models with drag handles or a typed factor.",
    ),
    v(
        184275443,
        "UltraWorks Shop Creator",
        "BusyCityGuy",
        "Builds in-game shops from a kit and a few settings.",
    ),
    v(
        10824128781,
        "Day/Night Cycle",
        "UkuMax",
        "Inserts a ready day and night cycle with one click.",
    ),
    v(
        135581141962270,
        "VFX Studio",
        "XxOmqxXNerd",
        "A toolkit for building visual effects.",
    ),
    v(
        217792838,
        "Model Reflect",
        "stravant",
        "Mirrors a model across a plane.",
    ),
    v(
        16976887711,
        "Scene Tools",
        "RoBuilders Plugins",
        "Tags props so they get life-like motion in a scene.",
    ),
];

/// The verified entry for `id`, which earns the "Runs in rbx-native" badge.
pub(super) fn verified(id: u64) -> Option<&'static Verified> {
    VERIFIED.iter().find(|entry| entry.id == id)
}
