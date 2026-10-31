//! Blocking HTTP client for the Roblox APIs a native editor needs: API key
//! introspection, universe/place metadata, public game listings, asset
//! delivery (anonymous first, Open Cloud key as fallback), model uploads, place publishing,
//! place version history, experience and place settings, experience icons, and the scope check behind the API key setup wizard,
//! Team Create's on/off switch and who is in a session (with the web sign-in),
//! Open Cloud v2 data stores (standard, ordered, and memory store sorted maps
//! and queues), a local mock Open Cloud server for them, and the scope list
//! for them. See `Client` for the entry point.

mod api_key;
mod archive;
mod asset_metadata;
mod assets;
mod avatar;
mod client;
mod create_asset;
pub mod datastore_mock;
mod datastores;
mod error;
mod experience_access;
mod experience_media;
mod experience_settings;
mod experiences;
pub mod expiry;
mod game_icon;
mod games;
pub mod heartbeats;
mod introspect;
mod inventory;
mod new_experience;
mod parallel;
mod places;
mod publish;
pub mod quick_login;
mod retry;
mod scopes;
mod security;
mod team_create;
mod thumbnails;
mod toolbox;
mod universes;
mod version_history;
mod web;

pub use api_key::{keyring_disabled, ApiKey};
pub use archive::{archived_experiences, set_experience_archived};
pub use asset_metadata::AssetMetadata;
pub use assets::AssetContent;
pub use avatar::{AssetType, Avatar, AvatarAsset, AvatarBodyColors, AvatarScales};
pub use client::Client;
pub use create_asset::ModelFile;
pub use datastores::{
    validate_entry, value_bytes, DataStoreInfo, Entry, EntryRef, EntryWrite, MapItem, MapItemWrite,
    OrderedEntry, OrderedQuery, Page, QueueItem, QueueRead, Revision, SortKey, StoreRef,
    ALL_SCOPES, MAX_ATTRIBUTES_CHARS, MAX_MEMORY_KEY_CHARS, MAX_MEMORY_TTL_SECONDS,
    MAX_MEMORY_VALUE_BYTES, MAX_NAME_CHARS, MAX_ORDERED_KEY_CHARS, MAX_VALUE_BYTES,
};
pub use error::{CloudError, KeyedRefusal};
pub use experience_access::{
    experience_genre, experience_public, set_experience_public, set_studio_api_access,
    studio_api_access,
};
pub use experience_media::{
    delete_experience_thumbnail, experience_icon, experience_thumbnails, upload_experience_icon,
    upload_experience_thumbnail, ExperienceIcon, Thumbnail, MAX_THUMBNAILS,
};
pub use experience_settings::{ExperienceSettings, ExperienceUpdate, PlaceSettings, PlaceUpdate};
pub use experiences::{Experience, Experiences, Group};
pub use game_icon::{GameIcon, Moderation};
pub use games::{Creator, CreatorKind, GameSummary};
pub use introspect::{KeyInfo, Scope};
pub use inventory::{InventoryAsset, InventoryPage};
pub use new_experience::{
    create_experience, creator_groups, name_experience, CreatedExperience, CreatorGroup,
};
pub use places::{place_id_from_link, PlaceSummary};
pub use publish::PublishMode;
pub use retry::rate_limit_waits;
pub use scopes::{
    check as check_scopes, Grant, KeyReport, Permission, ScopeCheck, DASHBOARD_API_KEYS_URL,
    PERMISSIONS,
};
pub use security::{mesh_image_apis, set_mesh_image_apis_allowed, MeshImageApis};
pub use team_create::{
    set_team_create_enabled, team_create_enabled, team_create_enabled_many, team_create_members,
    SessionMember,
};
pub use toolbox::{StoreAsset, StoreCategory, StoreCreator, StorePage, StoreSearch};
pub use universes::{Owner, Universe, Visibility};
pub use version_history::{PlaceVersion, VersionPage};
