//! The settings that aren't on the live experience: the instance
//! properties of the open place file ([`LocalField`]) and the ones Open
//! Cloud can't reach at all ([`Unreachable`]), each with the reason.

use super::Tab;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::shell::game_settings) enum LocalKind {
    Bool,
    Float {
        min: f32,
        max: f32,
    },
    /// The reflection database's enum of that name.
    Enum(&'static str),
}

/// An instance property of a service in the open place.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::shell::game_settings) struct LocalField {
    pub(in crate::shell::game_settings) tab: Tab,
    pub(in crate::shell::game_settings) class: &'static str,
    pub(in crate::shell::game_settings) property: &'static str,
    pub(in crate::shell::game_settings) label: &'static str,
    pub(in crate::shell::game_settings) description: &'static str,
    pub(in crate::shell::game_settings) kind: LocalKind,
}

const fn local(
    tab: Tab,
    class: &'static str,
    property: &'static str,
    label: &'static str,
    description: &'static str,
    kind: LocalKind,
) -> LocalField {
    LocalField {
        tab,
        class,
        property,
        label,
        description,
        kind,
    }
}

pub(in crate::shell::game_settings) const LOCAL_FIELDS: &[LocalField] = &[
    local(
        Tab::Communication,
        "VoiceChatService",
        "EnableDefaultVoice",
        "Default voice setup",
        "Creates the default audio inputs and emitters for each player.",
        LocalKind::Bool,
    ),
    local(
        Tab::Communication,
        "VoiceChatService",
        "UseAudioApi",
        "Audio API",
        "Whether voice runs through the audio instances (AudioDeviceInput and friends).",
        LocalKind::Enum("AudioApiRollout"),
    ),
    local(
        Tab::Communication,
        "VoiceChatService",
        "DefaultDistanceAttenuation",
        "Distance attenuation",
        "How the default voice setup fades with distance.",
        LocalKind::Enum("VoiceChatDistanceAttenuationType"),
    ),
    local(
        Tab::Permissions,
        "Players",
        "BanningEnabled",
        "Ban API",
        "Lets scripts ban players through Players:BanAsync.",
        LocalKind::Bool,
    ),
    local(
        Tab::Avatar,
        "StarterPlayer",
        "GameSettingsAvatar",
        "Avatar type",
        "The place file\u{2019}s copy of the experience\u{2019}s R6/R15 choice.",
        LocalKind::Enum("GameAvatarType"),
    ),
    local(
        Tab::Avatar,
        "StarterPlayer",
        "GameSettingsR15Collision",
        "Collision",
        "The place file\u{2019}s copy of the R15 collision setting.",
        LocalKind::Enum("R15CollisionType"),
    ),
    local(
        Tab::Avatar,
        "StarterPlayer",
        "LoadCharacterAppearance",
        "Load character appearance",
        "Dress characters in each player\u{2019}s own avatar.",
        LocalKind::Bool,
    ),
    local(
        Tab::Avatar,
        "StarterPlayer",
        "LoadCharacterLayeredClothing",
        "Layered clothing",
        "Whether characters load layered clothing.",
        LocalKind::Enum("LoadCharacterLayeredClothing"),
    ),
    local(
        Tab::Avatar,
        "StarterPlayer",
        "EnableDynamicHeads",
        "Dynamic heads",
        "Whether characters load animatable heads.",
        LocalKind::Enum("LoadDynamicHeads"),
    ),
    local(
        Tab::Avatar,
        "StarterPlayer",
        "UserEmotesEnabled",
        "Emotes",
        "Lets players play the emotes they own.",
        LocalKind::Bool,
    ),
    local(
        Tab::Avatar,
        "Players",
        "CharacterAutoLoads",
        "Characters auto-load",
        "Spawn each player\u{2019}s character without a script asking.",
        LocalKind::Bool,
    ),
    local(
        Tab::Avatar,
        "Players",
        "RespawnTime",
        "Respawn time",
        "Seconds between a character\u{2019}s death and its respawn.",
        LocalKind::Float {
            min: 0.,
            max: 3600.,
        },
    ),
    local(
        Tab::Security,
        "HttpService",
        "HttpEnabled",
        "Allow HTTP Requests",
        "Lets server scripts call web services through HttpService.",
        LocalKind::Bool,
    ),
    local(
        Tab::Security,
        "AssetService",
        "AllowInsertFreeAssets",
        "Allow Loading Third Party Assets",
        "Lets scripts insert assets this experience\u{2019}s owner doesn\u{2019}t own.",
        LocalKind::Bool,
    ),
];

/// A setting the real dialog has that this one can't reach, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell::game_settings) struct Unreachable {
    pub(in crate::shell::game_settings) tab: Tab,
    pub(in crate::shell::game_settings) label: &'static str,
    pub(in crate::shell::game_settings) reason: &'static str,
}

const NO_FIELD: &str = "Open Cloud has no field for it; set it on the Creator Hub.";
const COOKIE_ONLY: &str =
    "Roblox writes it through a website API an API key can\u{2019}t call; set it on the Creator Hub.";

pub(in crate::shell::game_settings) const UNREACHABLE: &[Unreachable] = &[
    Unreachable {
        tab: Tab::BasicInfo,
        label: "Genre",
        reason: COOKIE_ONLY,
    },
    Unreachable {
        tab: Tab::BasicInfo,
        label: "Icon and thumbnails",
        reason: "Only experimental legacy endpoints upload them; set them on the Creator Hub.",
    },
    Unreachable {
        tab: Tab::Communication,
        label: "Enable Camera",
        reason: NO_FIELD,
    },
    Unreachable {
        tab: Tab::Communication,
        label: "Enable Microphone",
        reason: NO_FIELD,
    },
    Unreachable {
        tab: Tab::Communication,
        label: "Chat and voice groups APIs",
        reason: NO_FIELD,
    },
    Unreachable {
        tab: Tab::Permissions,
        label: "Collaborators and group roles",
        reason: "Open Cloud only reads them, through an experimental legacy endpoint; edit them on the Creator Hub.",
    },
    Unreachable {
        tab: Tab::Permissions,
        label: "Friends only",
        reason: COOKIE_ONLY,
    },
    Unreachable {
        tab: Tab::Monetization,
        label: "Paid access",
        reason: COOKIE_ONLY,
    },
    Unreachable {
        tab: Tab::Monetization,
        label: "Passes and developer products",
        reason: "Open Cloud\u{2019}s endpoints for them are in beta and not used here; manage them on the Creator Hub.",
    },
    Unreachable {
        tab: Tab::Localization,
        label: "Source and supported languages",
        reason: "Only experimental legacy endpoints (legacy-universe:manage) reach them; set them on the Creator Hub.",
    },
    Unreachable {
        tab: Tab::Localization,
        label: "Automatic translation",
        reason: "Only experimental legacy endpoints (legacy-universe:manage) reach it; set it on the Creator Hub.",
    },
    Unreachable {
        tab: Tab::Avatar,
        label: "Live avatar settings",
        reason: "Scale, body, clothing, animation and collision live on Roblox behind a website API an API key can\u{2019}t call. The rows above are this place file\u{2019}s copies.",
    },
    Unreachable {
        tab: Tab::Security,
        label: "Enable Studio Access to API Services",
        reason: COOKIE_ONLY,
    },
    Unreachable {
        tab: Tab::Security,
        label: "Allow Third Party Sales and Teleports",
        reason: NO_FIELD,
    },
    Unreachable {
        tab: Tab::Security,
        label: "Allow Mesh / Image APIs",
        reason: COOKIE_ONLY,
    },
    Unreachable {
        tab: Tab::Places,
        label: "Server fill and Allow Copying",
        reason: COOKIE_ONLY,
    },
];
