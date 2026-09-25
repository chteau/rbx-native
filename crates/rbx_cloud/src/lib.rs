//! Blocking HTTP client for the Roblox APIs a native editor needs: API key
//! introspection, universe/place metadata, public game listings, asset
//! delivery (anonymous first, Open Cloud key as fallback), place publishing,
//! experience icons, and the scope check behind the API key setup wizard.
//! See `Client` for the entry point.

mod api_key;
mod assets;
mod client;
mod error;
mod experiences;
mod games;
mod introspect;
mod inventory;
mod places;
mod publish;
mod retry;
mod scopes;
mod thumbnails;
mod universes;

pub use api_key::ApiKey;
pub use assets::AssetContent;
pub use client::Client;
pub use error::CloudError;
pub use experiences::{Experience, Experiences, Group};
pub use games::{Creator, CreatorKind, GameSummary};
pub use introspect::{KeyInfo, Scope};
pub use places::place_id_from_link;
pub use publish::PublishMode;
pub use scopes::{
    check as check_scopes, Grant, KeyReport, Permission, ScopeCheck, DASHBOARD_API_KEYS_URL,
    PERMISSIONS,
};
pub use universes::{Owner, Universe, Visibility};
