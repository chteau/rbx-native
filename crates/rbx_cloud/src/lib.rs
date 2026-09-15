//! Blocking HTTP client for the Roblox APIs a native editor needs: API key
//! introspection, universe/place metadata, public game listings, asset
//! delivery (anonymous first, Open Cloud key as fallback), and place
//! publishing. See `Client` for the entry point.

mod api_key;
mod assets;
mod client;
mod error;
mod experiences;
mod games;
mod introspect;
mod publish;
mod universes;

pub use api_key::ApiKey;
pub use assets::AssetContent;
pub use client::Client;
pub use error::CloudError;
pub use experiences::Experience;
pub use games::{Creator, CreatorKind, GameSummary};
pub use introspect::{KeyInfo, Scope};
pub use publish::PublishMode;
pub use universes::{Owner, Universe, Visibility};
