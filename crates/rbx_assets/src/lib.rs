//! Resolution, caching and decoding of assets referenced by Roblox place
//! files (`rbxassetid://`, `rbxasset://`, legacy `roblox.com` URLs, ...).
//!
//! Entry points: [`AssetRef::parse`], [`sniff`], [`AssetCache`] and
//! [`AssetResolver`]. This crate never talks to the Roblox asset API itself —
//! [`AssetFetcher`] is the seam a real network client (elsewhere, backed by
//! `rbx_cloud`) plugs into.

mod asset_ref;
mod cache;
mod dds;
mod decode;
mod error;
mod native;
mod resolver;
mod sniff;
mod sober;

pub use asset_ref::AssetRef;
pub use cache::AssetCache;
pub use decode::decode_image;
pub use error::{AssetError, AssetRefError, CacheError, FetchError};
pub use native::NativeContent;
pub use resolver::{Asset, AssetFetcher, AssetResolver, MemoryFetcher};
pub use sniff::{sniff, AssetKind};
pub use sober::{Sober, SoberError};
