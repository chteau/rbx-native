//! Blocking HTTP client for the Roblox APIs a native editor needs: API key
//! introspection, universe/place metadata, public game listings, asset
//! delivery (anonymous first, Open Cloud key as fallback), model uploads, place publishing,
//! place version history, experience and place settings, experience icons, and the scope check behind the API key setup wizard,
//! Open Cloud v2 data stores (standard, ordered, and memory store sorted maps
//! and queues), a local mock Open Cloud server for them, and the scope list
//! for them. See `Client` for the entry point.

mod api_key;
mod assets;
mod avatar;
mod client;
mod create_asset;
pub mod datastore_mock;
mod datastores;
mod error;
mod experience_settings;
mod experiences;
mod games;
pub mod heartbeats;
mod introspect;
mod inventory;
mod places;
mod publish;
pub mod quick_login;
mod retry;
mod scopes;
mod thumbnails;
mod universes;
mod version_history;

pub use api_key::ApiKey;
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
pub use experience_settings::{ExperienceSettings, ExperienceUpdate, PlaceSettings, PlaceUpdate};
pub use experiences::{Experience, Experiences, Group};
pub use games::{Creator, CreatorKind, GameSummary};
pub use introspect::{KeyInfo, Scope};
pub use places::place_id_from_link;
pub use publish::PublishMode;
pub use retry::rate_limit_waits;
pub use scopes::{
    check as check_scopes, Grant, KeyReport, Permission, ScopeCheck, DASHBOARD_API_KEYS_URL,
    PERMISSIONS,
};
pub use universes::{Owner, Universe, Visibility};
pub use version_history::{PlaceVersion, VersionPage};
