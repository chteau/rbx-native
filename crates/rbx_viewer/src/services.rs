//! Roblox Studio's Explorer, as data: the order it lists services in and
//! which of Roblox's own services its default view hides. Shared by
//! `rbxstudio`'s Explorer and the browser build's (see `web`).

/// The order Studio lists services in — neither alphabetical nor the order the
/// file stores them in. Anything else a place has at its root comes after.
pub const SERVICE_ORDER: [&str; 14] = [
    "Workspace",
    "Players",
    "Lighting",
    "MaterialService",
    "ReplicatedFirst",
    "ReplicatedStorage",
    "ServerScriptService",
    "ServerStorage",
    "StarterGui",
    "StarterPack",
    "StarterPlayer",
    "Teams",
    "SoundService",
    "TextChatService",
];

/// Every root "service" class Roblox itself creates in a place file, whether
/// or not Studio's default Explorer view shows it (verified against every
/// root instance dump across this repo's fixtures). A root class absent from
/// this list is some real, user-placed instance — never one of the
/// deliberately-noisy internal ones — so the default filter below must never
/// hide it.
const KNOWN_SERVICES: [&str; 55] = [
    "AssetService",
    "Chat",
    "CollectionService",
    "ContextActionService",
    "CookiesService",
    "CSGDictionaryService",
    "DataStoreService",
    "Debris",
    "DevPackages",
    "GamePassService",
    "GuidRegistryService",
    "HttpService",
    "InsertService",
    "Instance",
    "Lighting",
    "LocalizationService",
    "LodDataService",
    "LuaWebService",
    "MaterialService",
    "NonReplicatedCSGDictionaryService",
    "Packages",
    "PermissionsService",
    "PhysicsService",
    "PlayerEmulatorService",
    "Players",
    "ProcessInstancePhysicsService",
    "ProximityPromptService",
    "ReplicatedFirst",
    "ReplicatedStorage",
    "ScriptService",
    "Selection",
    "SerializationService",
    "ServerPackages",
    "ServerScriptService",
    "ServerStorage",
    "ServiceVisibilityService",
    "SoundService",
    "StarterGui",
    "StarterPack",
    "StarterPlayer",
    "StudioData",
    "Teams",
    "TeleportService",
    "TestService",
    "TextChatService",
    "TimerService",
    "TouchInputService",
    "TweenService",
    "UGCAvatarService",
    "VideoCaptureService",
    "VideoService",
    "VirtualInputManager",
    "VoiceChatService",
    "VRService",
    "Workspace",
];

/// Where `class` sits in [`SERVICE_ORDER`], if it is one of those.
pub fn rank(class: &str) -> Option<usize> {
    SERVICE_ORDER.iter().position(|service| *service == class)
}

/// Whether a root of this class is one of the services this Explorer knows,
/// shown or not. Asked beside the dump's own `Service` tag because a few of
/// them (`Packages`, `SerializationService`) are not in the dump at all.
pub fn is_known_service(class: &str) -> bool {
    rank(class).is_some() || KNOWN_SERVICES.contains(&class)
}

/// Whether Studio's default Explorer view shows a root of this class without
/// the "show all services" toggle: its 14 fixed services, or anything that is
/// not one of Roblox's own well-known service classes at all.
pub fn is_default_visible(class: &str) -> bool {
    rank(class).is_some() || !KNOWN_SERVICES.contains(&class)
}
