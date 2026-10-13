//! Every setting the window shows, and where each one lives. Real Studio's
//! Experience Settings (creator-docs: `projects/index.md` names the tabs,
//! `chat/voice-chat.md`, `cloud-services/http-service.md`,
//! `studio/avatar-settings.md` and the place guides name their fields)
//! writes all of them to Roblox; an API key reaches only what Open Cloud's
//! universe and place resources expose (`reference/cloud/openapi.json`).
//! So each field is one of three kinds, and the window says which:
//!
//! - [`CloudField`]: read from and saved to the live experience.
//! - [`LocalField`]: an instance property in the open place file, saved
//!   with the file — Studio's dialog writes the same property, but here it
//!   reaches Roblox only when the place is published.
//! - [`Unreachable`]: listed with the reason, never pretended to.

/// The window's pages, in the order `projects/index.md` lists them, then
/// the two the dialog also has (Security, Places).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Tab {
    BasicInfo,
    Communication,
    Permissions,
    Monetization,
    Localization,
    Avatar,
    Security,
    Places,
}

impl Tab {
    pub(super) const ALL: [Tab; 8] = [
        Tab::BasicInfo,
        Tab::Communication,
        Tab::Permissions,
        Tab::Monetization,
        Tab::Localization,
        Tab::Avatar,
        Tab::Security,
        Tab::Places,
    ];

    pub(super) fn title(self) -> &'static str {
        match self {
            Tab::BasicInfo => "Basic Info",
            Tab::Communication => "Communication",
            Tab::Permissions => "Permissions",
            Tab::Monetization => "Monetization",
            Tab::Localization => "Localization",
            Tab::Avatar => "Avatar",
            Tab::Security => "Security",
            Tab::Places => "Places",
        }
    }

    /// A Lucide glyph's file name, for the sidebar.
    pub(super) fn glyph(self) -> &'static str {
        match self {
            Tab::BasicInfo => "info",
            Tab::Communication => "mic",
            Tab::Permissions => "users",
            Tab::Monetization => "coins",
            Tab::Localization => "languages",
            Tab::Avatar => "person-standing",
            Tab::Security => "shield",
            Tab::Places => "map",
        }
    }

    pub(super) fn subtitle(self) -> &'static str {
        match self {
            Tab::BasicInfo => "How the experience is listed, and which devices can join.",
            Tab::Communication => "Voice chat, and the voice defaults this place sets up.",
            Tab::Permissions => "Who can play, and who can edit.",
            Tab::Monetization => "Private servers and paid access.",
            Tab::Localization => "Languages and automatic translation.",
            Tab::Avatar => "The avatar defaults this place file carries.",
            Tab::Security => "What scripts in this place are allowed to reach.",
            Tab::Places => "The linked place's own name, description and server size.",
        }
    }
}

/// A setting on the live experience, through an API key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum CloudField {
    /// The root place's `displayName`, which Roblox mirrors as the
    /// experience's.
    Name,
    /// The root place's `description`, mirrored the same way.
    Description,
    AgeRating,
    Desktop,
    Mobile,
    Tablet,
    Console,
    Vr,
    VoiceChat,
    Visibility,
    PrivateServers,
    PlaceName,
    PlaceDescription,
    ServerSize,
}

impl CloudField {
    pub(super) const ALL: [CloudField; 14] = [
        CloudField::Name,
        CloudField::Description,
        CloudField::AgeRating,
        CloudField::Desktop,
        CloudField::Mobile,
        CloudField::Tablet,
        CloudField::Console,
        CloudField::Vr,
        CloudField::VoiceChat,
        CloudField::Visibility,
        CloudField::PrivateServers,
        CloudField::PlaceName,
        CloudField::PlaceDescription,
        CloudField::ServerSize,
    ];

    pub(super) fn tab(self) -> Tab {
        match self {
            CloudField::Name
            | CloudField::Description
            | CloudField::AgeRating
            | CloudField::Desktop
            | CloudField::Mobile
            | CloudField::Tablet
            | CloudField::Console
            | CloudField::Vr => Tab::BasicInfo,
            CloudField::VoiceChat => Tab::Communication,
            CloudField::Visibility => Tab::Permissions,
            CloudField::PrivateServers => Tab::Monetization,
            CloudField::PlaceName | CloudField::PlaceDescription | CloudField::ServerSize => {
                Tab::Places
            }
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            CloudField::Name => "Name",
            CloudField::Description => "Description",
            CloudField::AgeRating => "Maturity rating",
            CloudField::Desktop => "Computer",
            CloudField::Mobile => "Phone",
            CloudField::Tablet => "Tablet",
            CloudField::Console => "Console",
            CloudField::Vr => "VR",
            CloudField::VoiceChat => "Enable Voice Chat",
            CloudField::Visibility => "Playability",
            CloudField::PrivateServers => "Private Servers",
            CloudField::PlaceName => "Place name",
            CloudField::PlaceDescription => "Place description",
            CloudField::ServerSize => "Max players",
        }
    }

    pub(super) fn description(self) -> &'static str {
        match self {
            CloudField::Name | CloudField::Description => {
                "Written to the root place; Roblox lists the experience under it."
            }
            CloudField::AgeRating => "Set by Roblox from the maturity questionnaire.",
            CloudField::Desktop
            | CloudField::Mobile
            | CloudField::Tablet
            | CloudField::Console
            | CloudField::Vr => "Whether players on this device can join.",
            CloudField::VoiceChat => "Running servers keep their old setting until they restart.",
            CloudField::Visibility => {
                "Open Cloud only reads this; make the experience public or private on the Creator Hub."
            }
            CloudField::PrivateServers => {
                "Turning them off shuts every private server; a new price cancels their subscriptions."
            }
            CloudField::PlaceName | CloudField::PlaceDescription => {
                "For the root place, the same as Basic Info\u{2019}s."
            }
            CloudField::ServerSize => "Players per server. Voice chat needs 100 or fewer.",
        }
    }

    /// Shown, never edited: Open Cloud marks it output-only.
    pub(super) fn read_only(self) -> bool {
        matches!(self, CloudField::AgeRating | CloudField::Visibility)
    }

    /// The API key scope a save of this field needs.
    pub(super) fn scope(self) -> &'static str {
        match self {
            CloudField::Name
            | CloudField::Description
            | CloudField::PlaceName
            | CloudField::PlaceDescription
            | CloudField::ServerSize => "universe.place:write",
            _ => "universe:write",
        }
    }
}

mod place;

pub(super) use place::{LocalField, LocalKind, LOCAL_FIELDS, UNREACHABLE};
