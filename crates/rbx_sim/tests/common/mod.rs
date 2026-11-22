//! Helpers the Session test binaries share.

/// The client's warning for an animation it could not load: these runs
/// have no network, so the stock Animate's animations load only when the
/// machine's asset cache happens to hold them, on a loader thread's
/// timing. What a test prints never depends on it.
pub fn animation_warning(line: &str) -> bool {
    line.starts_with("Failed to load animation with sanitized ID ")
}
