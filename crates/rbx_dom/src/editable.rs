//! What a running game paints or builds in memory, an `EditableImage`'s
//! pixels and an `EditableMesh`'s triangles, bound to an instance through a
//! `Content::Object` property. The script VM that makes one publishes it here
//! under a referent no DOM allocates (they count down from the top), so a
//! renderer reading any copy of the DOM can find it. Each edit moves the
//! object's `version` and the shared [`generation`], so a reader re-uploads
//! only what changed and can tell cheaply that nothing did.

use std::any::Any;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use crate::Ref;

/// RGBA8 pixels, row by row, as `EditableImage:ReadPixelsBuffer` hands them.
#[derive(Debug, Default, Clone)]
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub version: u64,
}

/// A mesh as unshared triangles: three corners per face, each with its own
/// normal, UV and colour (alpha last).
#[derive(Debug, Default, Clone)]
pub struct Triangles {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub colors: Vec<[f32; 4]>,
}

/// An `EditableMesh` as its VM keeps it, flattened only when a reader asks:
/// building a mesh vertex by vertex must not re-flatten it on every call.
pub trait MeshSource: Send + Sync {
    fn version(&self) -> u64;
    fn triangles(&self) -> Triangles;
    fn into_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync>;
}

#[derive(Clone)]
pub enum Editable {
    Image(Arc<Mutex<Pixels>>),
    Mesh(Arc<dyn MeshSource>),
}

impl Editable {
    pub fn version(&self) -> u64 {
        match self {
            Editable::Image(pixels) => lock(pixels).version,
            Editable::Mesh(mesh) => mesh.version(),
        }
    }
}

/// The property a running game's `VideoFrame` or `VideoDisplay` holds its
/// decoded picture under, a `Content::Object` naming an editable image. No
/// Roblox class has it, and it only lives in a running game's DOM.
pub const VIDEO_PICTURE: &str = "RbxNativeVideoPicture";

static NEXT: AtomicU32 = AtomicU32::new(u32::MAX);
static GENERATION: AtomicU64 = AtomicU64::new(0);

fn table() -> MutexGuard<'static, HashMap<Ref, Editable>> {
    static TABLE: OnceLock<Mutex<HashMap<Ref, Editable>>> = OnceLock::new();
    lock(TABLE.get_or_init(Default::default))
}

/// A poisoned lock still holds usable pixels: a panicking script thread
/// must not take the renderer down with it.
pub fn lock<T: ?Sized>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Publishes `editable` under a fresh referent.
pub fn publish(editable: Editable) -> Ref {
    let referent = Ref::new(NEXT.fetch_sub(1, Ordering::Relaxed));
    table().insert(referent, editable);
    touch();
    referent
}

pub fn withdraw(referent: Ref) {
    if table().remove(&referent).is_some() {
        touch();
    }
}

pub fn get(referent: Ref) -> Option<Editable> {
    table().get(&referent).cloned()
}

/// Called after every edit.
pub fn touch() {
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

/// Moves on every publish, withdrawal and edit of any editable object.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}
