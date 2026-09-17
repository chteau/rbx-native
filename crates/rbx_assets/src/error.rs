//! Error types for asset reference parsing, caching, resolution and decoding.

use thiserror::Error;

/// Errors that can occur while parsing an asset reference string.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum AssetRefError {
    #[error("unrecognized asset reference scheme: {0:?}")]
    UnknownScheme(String),

    #[error("asset reference has no numeric id: {0:?}")]
    MissingId(String),

    #[error("asset reference id is not a valid u64: {0:?}")]
    InvalidId(String),
}

/// Errors that can occur while reading from or writing to the on-disk cache.
#[derive(Debug, Error)]
pub enum CacheError {
    #[error("could not determine a cache directory (no XDG_CACHE_HOME or HOME)")]
    NoCacheDir,

    #[error("failed to create cache directory {path}: {source}")]
    CreateDir {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to write cache entry {path}: {source}")]
    Write {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to read cache entry {path}: {source}")]
    Read {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Errors that can occur while fetching an asset by id from a remote source.
///
/// The real network fetcher lives outside this crate (in the viewer, backed by
/// `rbx_cloud`); this error type is the boundary it must implement against.
#[derive(Debug, Error)]
pub enum FetchError {
    #[error("asset {0} was not found")]
    NotFound(u64),

    /// The service answered, and its answer is final: the asset is there but
    /// this caller may not have it, or the request itself was wrong. Asking
    /// again with the same credentials gets the same answer.
    #[error("fetching asset {id} was refused: {message}")]
    Refused { id: u64, message: String },

    /// The request did not get an answer worth keeping — a rate limit, an
    /// outage, a dropped connection. About the machine and the moment, not
    /// about the asset, so a later try may well succeed.
    #[error("fetching asset {id} failed: {message}")]
    Other { id: u64, message: String },
}

/// Errors that can occur while resolving an [`crate::AssetRef`] to bytes, or
/// while downloading and extracting native Studio content packages.
#[derive(Debug, Error)]
pub enum AssetError {
    #[error("cannot resolve an empty asset reference")]
    Empty,

    #[error(transparent)]
    Cache(#[from] CacheError),

    #[error(transparent)]
    Fetch(#[from] FetchError),

    #[error("native content network request failed: {0}")]
    Network(String),

    #[error("native content package {0:?} is unknown for path {1:?}")]
    UnknownNativePackage(&'static str, String),

    #[error("native content package for path {0:?} does not contain the requested file")]
    NativeFileNotFound(String),

    #[error("failed to read zip archive: {0}")]
    Zip(String),

    #[error("failed to decode image: {0}")]
    ImageDecode(String),

    #[error(
        "package {package} is {size_mb:.1} MiB, above the {limit_mb} MiB safety limit; refusing to download"
    )]
    PackageTooLarge {
        package: String,
        size_mb: f64,
        limit_mb: u64,
    },
}
