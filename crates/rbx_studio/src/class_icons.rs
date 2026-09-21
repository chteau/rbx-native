//! This project's own class icons — flat, multi-colour SVGs shipped in
//! `assets/icons/default/dark` and `assets/icons/default/light` (spec'd in
//! `assets/icons/README.md`, gitignored working notes, not a shipped asset)
//! — replacing the sprite sheet Roblox's own Studio ships, which this
//! project no longer downloads or draws.
//!
//! Rasterized ourselves with `resvg` rather than painted through GPUI's own
//! `svg()` element: that element is a monochrome icon renderer (it always
//! recolors its SVG to one flat `text_color`, discarding whatever fill the
//! file itself carries — fine for Lucide's single-color glyphs, wrong for
//! this kit's palette), so the sliced-sprite path the old Roblox sheet used
//! (`render_image::to_render_image`, painted with `img()`) is kept and fed
//! from a rasterized SVG instead of a downloaded PNG tile.
//!
//! Both variants are embedded at compile time; [`IconPack`] (an editor
//! setting, see `settings::Settings::icon_pack`) only picks which one
//! [`icon_tile`] reads from — no rebuild needed to switch. A user's own pack
//! (`crate::packs`) is layered over either: [`set_user_pack`] installs it, and
//! whatever it leaves out is still drawn from the built-in kit.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use gpui_kit::RenderImage;
use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg::{Options, Tree};

use crate::packs::IconOverlay;
use crate::render_image::to_render_image;

mod tint;
pub(crate) use tint::tint;

/// Every SVG in `assets/icons/default/dark`, embedded at compile time.
#[derive(rust_embed::RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../assets/icons/default/dark"]
struct DefaultIcons;

/// Every SVG in `assets/icons/default/light`, embedded at compile time
/// alongside [`DefaultIcons`] — [`IconPack`] picks between the two at
/// lookup time, so both ship in the binary regardless of which is active.
#[derive(rust_embed::RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../assets/icons/default/light"]
struct LightIcons;

/// Which of the kit's two equal-sized variants is currently drawn — a
/// persisted editor setting (see `settings::Settings::icon_pack`), not a
/// build-time choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) enum IconPack {
    #[default]
    Dark,
    Light,
}

/// Every icon in the kit is authored on a 16x16 `viewBox` (see
/// `assets/icons/README.md`'s "Canvas" section); rasterized at 2x for a
/// sharp downscale to the Explorer's `CLASS_ICON_SIZE`.
const RENDER_SIZE: u32 = 32;

/// `ClassName -> icon slug`, extracted from the class icon kit's own spec
/// (`assets/icons/README.md`'s "Tiles" section, which lists each SVG's
/// filename against the classes it covers) — 329 classes onto 142 of the
/// kit's 152 tiles; a class missing here falls back to a Lucide glyph
/// (`explorer::icon`), same as a class undocumented in Roblox's own sheet
/// used to.
///
/// The spec covers what Roblox's own `ExplorerImageIndex` metadata covers,
/// which is not everything a creator inserts — the insert picker (see
/// `shell::explorer_edit::picker`) lists every browsable class, so classes
/// the metadata skips show up there with nothing but a Lucide glyph. Those
/// are mapped here too: to the nearest tile in the kit's own families where
/// one fits (a `FileMesh` is a mesh, a `KeyframeSequence` is an animation),
/// and to one of the five tiles authored beyond the sheet's 147 —
/// `style-sheet`, `style-rule`, `style-link`, `intersect-operation`,
/// `body-colors` — where none did.
const CLASS_ICON_SLUGS: &[(&str, &str)] = &[
    ("Accessory", "accessory"),
    ("AccessoryDescription", "humanoid-description"),
    ("Accoutrement", "accessory"),
    ("Actor", "actor"),
    ("AdGui", "ad-gui"),
    ("AdPortal", "ad-portal"),
    ("AlignOrientation", "align-orientation"),
    ("AlignPosition", "align-position"),
    ("AngularVelocity", "angular-velocity"),
    ("Animation", "animation"),
    ("AnimationController", "animation"),
    ("AnimationRigData", "animation"),
    ("AnimationTrack", "animation"),
    ("Animator", "animation"),
    ("ArcHandles", "arc-handles"),
    ("Atmosphere", "sky"),
    ("Attachment", "attachment"),
    ("AudioAnalyzer", "sound"),
    ("AudioChannelMixer", "sound"),
    ("AudioChannelSplitter", "sound"),
    ("AudioChorus", "sound"),
    ("AudioCompressor", "sound"),
    ("AudioDeviceInput", "sound"),
    ("AudioDeviceOutput", "sound"),
    ("AudioDistortion", "sound"),
    ("AudioEcho", "sound"),
    ("AudioEmitter", "sound"),
    ("AudioEqualizer", "sound"),
    ("AudioFDNReverb", "sound"),
    ("AudioFader", "sound"),
    ("AudioFilter", "sound"),
    ("AudioFlanger", "sound"),
    ("AudioGate", "sound"),
    ("AudioLimiter", "sound"),
    ("AudioListener", "sound"),
    ("AudioPitchShifter", "sound"),
    ("AudioPlayer", "sound"),
    ("AudioRecorder", "sound"),
    ("AudioReverb", "sound"),
    ("AudioSearchParams", "sound"),
    ("AudioSpeechToText", "sound"),
    ("AudioStreamReader", "sound"),
    ("AudioStreamWriter", "sound"),
    ("AudioTextToSpeech", "sound"),
    ("AudioTremolo", "sound"),
    ("AudioWindSynthesizer", "sound"),
    ("AuroraScript", "module-script"),
    ("Backpack", "backpack"),
    ("BallSocketConstraint", "ball-socket-constraint"),
    ("Beam", "beam"),
    ("BillboardGui", "surface-gui"),
    ("BinaryStringValue", "value"),
    ("BindableEvent", "bindable-event"),
    ("BindableFunction", "bindable-function"),
    ("BlockMesh", "mesh"),
    ("BloomEffect", "post-effect"),
    ("BlurEffect", "post-effect"),
    ("BodyAngularVelocity", "body-mover"),
    ("BodyColors", "body-colors"),
    ("BodyForce", "body-mover"),
    ("BodyGyro", "body-mover"),
    ("BodyPartDescription", "humanoid-description"),
    ("BodyPosition", "body-mover"),
    ("BodyThrust", "body-mover"),
    ("BodyVelocity", "body-mover"),
    ("Bone", "bone"),
    ("BoolValue", "value"),
    ("BoxHandleAdornment", "box-handle-adornment"),
    ("BrickColorValue", "value"),
    ("BubbleChatMessageProperties", "chat-window-configuration"),
    ("CFrameValue", "value"),
    ("Camera", "camera"),
    ("CanvasGroup", "frame"),
    ("ChannelTabsConfiguration", "chat-window-configuration"),
    ("CharacterMesh", "animation"),
    ("Chat", "message"),
    ("ChatInputBarConfiguration", "chat-input-bar-configuration"),
    ("ChatService", "message"),
    ("ChatWindowConfiguration", "chat-window-configuration"),
    ("ChorusSoundEffect", "sound-effect"),
    ("ClickDetector", "click-detector"),
    ("Clouds", "sky"),
    ("Color3Value", "value"),
    ("ColorCorrectionEffect", "post-effect"),
    ("ColorGradingEffect", "post-effect"),
    ("CompressorSoundEffect", "sound-effect"),
    ("ConeHandleAdornment", "cone-handle-adornment"),
    ("Configuration", "configuration"),
    ("Constraint", "ball-socket-constraint"),
    ("CoreGui", "gui-container"),
    ("CorePackages", "backpack"),
    ("CornerWedgePart", "part"),
    ("CurveAnimation", "animation"),
    ("CustomEvent", "value"),
    ("CustomEventReceiver", "value"),
    ("CylinderHandleAdornment", "cylinder-handle-adornment"),
    ("CylinderMesh", "mesh"),
    ("CylindricalConstraint", "cylindrical-constraint"),
    ("Debris", "debris"),
    ("Decal", "decal"),
    ("DepthOfFieldEffect", "post-effect"),
    ("Dialog", "dialog"),
    ("DialogChoice", "dialog-choice"),
    ("DistortionSoundEffect", "sound-effect"),
    ("DoubleConstrainedValue", "value"),
    ("DragDetector", "click-detector"),
    ("DynamicMesh", "mesh"),
    ("EchoSoundEffect", "sound-effect"),
    ("EqualizerSoundEffect", "sound-effect"),
    ("EulerRotationCurve", "animation"),
    ("Explosion", "explosion"),
    ("FaceControls", "face-controls"),
    ("FileMesh", "mesh"),
    ("Fire", "fire"),
    ("Flag", "flag"),
    ("FlagStand", "flag-stand"),
    ("FlangeSoundEffect", "sound-effect"),
    ("FloatCurve", "animation"),
    ("FloorWire", "value"),
    ("Folder", "folder"),
    ("ForceField", "force-field"),
    ("Frame", "frame"),
    ("GeneratedFolder", "folder"),
    ("GuiButton", "image-button"),
    ("GuiMain", "screen-gui"),
    ("HandRigDescription", "handles"),
    ("Handles", "handles"),
    ("Hat", "hat"),
    ("Highlight", "highlight"),
    ("HingeConstraint", "hinge-constraint"),
    ("Hint", "message"),
    ("HopperBin", "hopper-bin"),
    ("Humanoid", "humanoid"),
    ("HumanoidController", "humanoid"),
    ("HumanoidDescription", "humanoid-description"),
    ("HumanoidRigDescription", "handles"),
    ("IKControl", "handles"),
    ("ImageButton", "image-button"),
    ("ImageHandleAdornment", "image-handle-adornment"),
    ("ImageLabel", "image-label"),
    ("IntConstrainedValue", "value"),
    ("IntValue", "value"),
    ("IntersectOperation", "intersect-operation"),
    ("JointInstance", "weld"),
    ("Keyframe", "animation"),
    ("KeyframeMarker", "animation"),
    ("KeyframeSequence", "animation"),
    ("Light", "light"),
    ("Lighting", "light"),
    ("LineForce", "line-force"),
    ("LineHandleAdornment", "line-handle-adornment"),
    ("LinearVelocity", "linear-velocity"),
    ("LocalScript", "local-script"),
    ("LocalizationService", "localization-service"),
    ("LocalizationTable", "localization-table"),
    ("MakeupDescription", "humanoid-description"),
    ("MarkerCurve", "animation"),
    ("MarketplaceService", "gui-container"),
    ("MaterialService", "material-service"),
    ("MaterialVariant", "material-variant"),
    ("MeshPart", "union-operation"),
    ("Message", "message"),
    ("Model", "model"),
    ("ModuleScript", "module-script"),
    ("Motor", "motor6d"),
    ("Motor6D", "motor6d"),
    ("NegateOperation", "negate-operation"),
    ("NetworkClient", "network-client"),
    ("NetworkReplicator", "network-replicator"),
    ("NetworkServer", "network-server"),
    ("NoCollisionConstraint", "no-collision-constraint"),
    ("NumberPose", "animation"),
    ("NumberValue", "value"),
    ("ObjectValue", "value"),
    ("PackageLink", "package-link"),
    ("Pants", "pants"),
    ("ParabolaAdornment", "line-handle-adornment"),
    ("ParallelRampPart", "part"),
    ("Part", "part"),
    ("PartOperation", "union-operation"),
    ("PartPairLasso", "lasso"),
    ("ParticleEmitter", "particle-emitter"),
    ("PathfindingLink", "pathfinding-link"),
    ("PathfindingModifier", "pathfinding-modifier"),
    ("PitchShiftSoundEffect", "sound-effect"),
    ("Plane", "plane-constraint"),
    ("PlaneConstraint", "plane-constraint"),
    ("Platform", "seat"),
    ("PlatformLibraries", "gui-container"),
    ("Player", "player"),
    ("PlayerGui", "gui-container"),
    ("PlayerScripts", "player-scripts"),
    ("Players", "players"),
    ("Plugin", "ball-socket-constraint"),
    ("PluginDebugService", "gui-container"),
    ("PluginGuiService", "gui-container"),
    ("PointLight", "light"),
    ("Pose", "animation"),
    ("PoseBase", "animation"),
    ("Preloaded", "replicated-storage"),
    ("PrismPart", "part"),
    ("PrismaticConstraint", "prismatic-constraint"),
    ("ProximityPrompt", "proximity-prompt"),
    ("PyramidHandleAdornment", "pyramid-handle-adornment"),
    ("PyramidPart", "part"),
    ("RayValue", "value"),
    ("RemoteEvent", "remote-event"),
    ("RemoteFunction", "remote-function"),
    ("RenderingTest", "camera"),
    ("ReplicatedFirst", "replicated-storage"),
    ("ReplicatedStorage", "replicated-storage"),
    ("ReverbSoundEffect", "sound-effect"),
    ("RightAngleRampPart", "part"),
    ("RigidConstraint", "rigid-constraint"),
    ("RobloxPluginGuiService", "gui-container"),
    ("RocketPropulsion", "body-mover"),
    ("RodConstraint", "rod-constraint"),
    ("RopeConstraint", "rope-constraint"),
    ("RotationCurve", "animation"),
    ("ScreenGui", "screen-gui"),
    ("Script", "script"),
    ("ScrollingFrame", "frame"),
    ("Seat", "seat"),
    ("SelectionBox", "selection-box"),
    ("SelectionPartLasso", "lasso"),
    ("SelectionPointLasso", "lasso"),
    ("SelectionSphere", "selection-box"),
    ("ServerScriptService", "server-script-service"),
    ("ServerStorage", "server-storage"),
    ("Shirt", "shirt"),
    ("ShirtGraphic", "shirt-graphic"),
    ("SkateboardController", "humanoid"),
    ("SkateboardPlatform", "seat"),
    ("Sky", "sky"),
    ("SlidingBallConstraint", "prismatic-constraint"),
    ("Smoke", "smoke"),
    ("Snap", "weld"),
    ("Sound", "sound"),
    ("SoundGroup", "sound-group"),
    ("SoundService", "sound-service"),
    ("Sparkles", "sparkles"),
    ("SpawnLocation", "spawn-location"),
    ("SpecialMesh", "mesh"),
    ("SphereHandleAdornment", "sphere-handle-adornment"),
    ("SpotLight", "light"),
    ("SpringConstraint", "spring-constraint"),
    ("StandalonePluginScripts", "player-scripts"),
    ("StarterCharacterScripts", "player-scripts"),
    ("StarterGear", "backpack"),
    ("StarterGui", "gui-container"),
    ("StarterPack", "backpack"),
    ("StarterPlayer", "starter-player"),
    ("StarterPlayerScripts", "player-scripts"),
    ("Status", "model"),
    ("StringValue", "value"),
    ("StyleBase", "style-sheet"),
    ("StyleDerive", "style-link"),
    ("StyleLink", "style-link"),
    ("StyleRule", "style-rule"),
    ("StyleSheet", "style-sheet"),
    ("SunRaysEffect", "post-effect"),
    ("SurfaceAppearance", "texture"),
    ("SurfaceGui", "surface-gui"),
    ("SurfaceGuiBase", "surface-gui"),
    ("SurfaceLight", "light"),
    ("SurfaceSelection", "surface-selection"),
    ("Team", "team"),
    ("Teams", "teams"),
    ("Terrain", "terrain"),
    ("TerrainDetail", "terrain-detail"),
    ("TerrainRegion", "terrain"),
    ("TestService", "test-service"),
    ("TextBox", "text-button"),
    ("TextButton", "text-button"),
    ("TextChannel", "text-channel"),
    ("TextChatCommand", "text-chat-command"),
    ("TextChatMessageProperties", "chat-window-configuration"),
    ("TextChatService", "text-chat-service"),
    ("TextLabel", "text-label"),
    ("TextSource", "text-source"),
    ("Texture", "texture"),
    ("Tool", "tool"),
    ("Torque", "angular-velocity"),
    ("TorsionSpringConstraint", "torsion-spring-constraint"),
    ("TouchTransmitter", "force-field"),
    ("TrackerStreamAnimation", "animation"),
    ("Trail", "trail"),
    ("TremoloSoundEffect", "sound-effect"),
    ("TrussPart", "part"),
    ("UIAspectRatioConstraint", "ui-constraint"),
    ("UICorner", "ui-constraint"),
    ("UIDragDetector", "click-detector"),
    ("UIFlexItem", "ui-constraint"),
    ("UIGradient", "ui-constraint"),
    ("UIGridLayout", "ui-constraint"),
    ("UIListLayout", "ui-constraint"),
    ("UIPadding", "ui-constraint"),
    ("UIPageLayout", "ui-constraint"),
    ("UIScale", "ui-constraint"),
    ("UIShadow", "ui-constraint"),
    ("UISizeConstraint", "ui-constraint"),
    ("UIStroke", "ui-constraint"),
    ("UITableLayout", "ui-constraint"),
    ("UITextSizeConstraint", "ui-constraint"),
    ("UnionOperation", "union-operation"),
    ("UniversalConstraint", "universal-constraint"),
    ("UnreliableRemoteEvent", "remote-event"),
    ("ValueBase", "value"),
    ("Vector3Curve", "animation"),
    ("Vector3Value", "value"),
    ("VectorForce", "vector-force"),
    ("VehicleController", "humanoid"),
    ("VehicleSeat", "seat"),
    ("VelocityMotor", "motor6d"),
    ("VideoDisplay", "sound"),
    ("VideoFrame", "video-frame"),
    ("VideoPlayer", "sound"),
    ("ViewportFrame", "image-button"),
    ("VoiceChatService", "voice-chat-service"),
    ("WedgePart", "part"),
    ("Weld", "weld"),
    ("WeldConstraint", "weld-constraint"),
    ("Wire", "sound"),
    ("WireframeHandleAdornment", "actor"),
    ("Workspace", "workspace"),
    ("WorldModel", "workspace"),
    ("WrapDeformer", "wrap-target"),
    ("WrapLayer", "wrap-layer"),
    ("WrapTarget", "wrap-target"),
];

/// The rasterized icon for `class` from the requested `pack`, or `None` for
/// a class the icon kit doesn't cover (falls back to a Lucide glyph — see
/// `explorer::resolve_icon`) or whose SVG failed to parse (a build-time
/// invariant, not a runtime one: every file under both `assets/icons/default`
/// variants is checked by this module's own tests).
pub(crate) fn icon_tile(class: &str, pack: IconPack) -> Option<Arc<RenderImage>> {
    if let Some(hit) = TILE_CACHE
        .read()
        .ok()
        .and_then(|cache| cache.get(pack, class))
    {
        return hit;
    }
    let installed = USER_PACK.read().ok().and_then(|pack| pack.clone());
    let tile = icon_tile_over(class, pack, installed.as_deref());
    if let Ok(mut cache) = TILE_CACHE.write() {
        cache.insert(pack, class, tile.clone());
    }
    tile
}

/// Every tile resolved so far, keyed by the pack it was read from.
///
/// [`rasterize`] parses an SVG and renders a pixmap on every call, which is
/// fine once per class per Explorer rebuild but not once per row per frame —
/// the insert picker (see `shell::explorer_edit::picker`) lists hundreds of
/// classes and rebuilds its list on every keystroke. The `None`s are cached
/// too: a class the kit does not cover is the *common* case there, and
/// re-deciding it would re-walk `CLASS_ICON_SLUGS` each time.
///
/// Keyed by *class* rather than by slug, which does mean two classes
/// sharing a tile each rasterize it once: an installed pack is allowed to
/// cover a class the kit does not (see [`IconOverlay::svg`]), so the class
/// is the only key that stays correct once one is layered on.
///
/// Bounded by the table itself — at most one entry per mapped class per
/// variant, a 32x32 RGBA tile each — so it needs no eviction.
/// [`set_user_pack`] is the only thing that can invalidate it, and clears
/// it.
static TILE_CACHE: RwLock<TileCache> = RwLock::new(TileCache::new());

/// [`icon_tile`] against an explicit overlay rather than the installed one, so
/// the precedence can be tested without touching process-wide state.
fn icon_tile_over(
    class: &str,
    pack: IconPack,
    overlay: Option<&IconOverlay>,
) -> Option<Arc<RenderImage>> {
    let slug = CLASS_ICON_SLUGS
        .iter()
        .find(|(name, _)| *name == class)
        .map(|(_, slug)| *slug);

    // The installed pack goes first, and falls through on a drawing that will
    // not parse rather than blanking the icon: it is somebody else's file.
    if let Some(image) = overlay
        .and_then(|overlay| overlay.svg(class, slug))
        .and_then(|svg| rasterize(&svg))
    {
        return Some(image);
    }

    let slug = slug?;
    let file = match pack {
        IconPack::Dark => DefaultIcons::get(&format!("{slug}.svg")),
        IconPack::Light => LightIcons::get(&format!("{slug}.svg")),
    }?;
    rasterize(&file.data)
}

/// The user's installed icon pack, drawn over the built-in kit — see
/// `crate::packs`. Process-wide because the Explorer resolves icons deep
/// inside row construction with no handle to the editor's state; set once at
/// startup and again whenever the Explorer's menu picks another.
static USER_PACK: RwLock<Option<Arc<IconOverlay>>> = RwLock::new(None);

/// Installs `pack` as the overlay, or removes it with `None`. The caller
/// rebuilds whatever already resolved an icon.
pub(crate) fn set_user_pack(pack: Option<IconOverlay>) {
    if let Ok(mut slot) = USER_PACK.write() {
        *slot = pack.map(Arc::new);
    }
    // Every cached tile was resolved against the overlay that just went
    // away, including the misses — a pack that covers a class the kit does
    // not would otherwise stay invisible until the next launch.
    if let Ok(mut cache) = TILE_CACHE.write() {
        cache.clear();
    }
}

/// [`TILE_CACHE`]'s map: one class-keyed map per variant, rather than one
/// map keyed by the pair, so a lookup borrows the class name instead of
/// allocating a `String` to build a tuple key with.
struct TileCache {
    dark: Option<HashMap<String, Option<Arc<RenderImage>>>>,
    light: Option<HashMap<String, Option<Arc<RenderImage>>>>,
}

impl TileCache {
    const fn new() -> Self {
        TileCache {
            dark: None,
            light: None,
        }
    }

    fn of(&self, pack: IconPack) -> &Option<HashMap<String, Option<Arc<RenderImage>>>> {
        match pack {
            IconPack::Dark => &self.dark,
            IconPack::Light => &self.light,
        }
    }

    /// `Some(hit)` only when this class has been resolved before — the outer
    /// `Option` is "have we looked", the inner one "does the kit cover it".
    fn get(&self, pack: IconPack, class: &str) -> Option<Option<Arc<RenderImage>>> {
        self.of(pack).as_ref()?.get(class).cloned()
    }

    fn insert(&mut self, pack: IconPack, class: &str, tile: Option<Arc<RenderImage>>) {
        let slot = match pack {
            IconPack::Dark => &mut self.dark,
            IconPack::Light => &mut self.light,
        };
        slot.get_or_insert_with(HashMap::new)
            .insert(class.to_owned(), tile);
    }

    fn clear(&mut self) {
        self.dark = None;
        self.light = None;
    }
}

/// Renders `svg` to a square RGBA tile.
///
/// The kit is authored on a 16x16 canvas, but the scale is taken from the
/// document's own size so a pack drawn on 24x24 or 32x32 fills the tile
/// instead of being cropped to its top-left corner, and a drawing that is not
/// square is centred on the shorter axis rather than pinned to the top-left.
/// (`usvg` refuses a document with no size, so the divisor is never zero.)
///
/// `pub(crate)`: also `action_icons`'s own rasterizer, for the ribbon's
/// action-icon kit (`assets/icons/actions`) — same 16x16 canvas, same
/// premultiplied-alpha fixup, no reason for a second copy of either.
pub(crate) fn rasterize(svg: &[u8]) -> Option<Arc<RenderImage>> {
    let tree = Tree::from_data(svg, &Options::default()).ok()?;
    let mut pixmap = Pixmap::new(RENDER_SIZE, RENDER_SIZE)?;
    let size = tree.size();
    let tile = RENDER_SIZE as f32;
    let scale = tile / size.width().max(size.height());
    let (x, y) = (
        (tile - size.width() * scale) / 2.0,
        (tile - size.height() * scale) / 2.0,
    );
    resvg::render(
        &tree,
        Transform::from_row(scale, 0.0, 0.0, scale, x, y),
        &mut pixmap.as_mut(),
    );

    // `Pixmap` is premultiplied alpha; `to_render_image`'s consumers (decoded
    // PNG tiles, wgpu readback frames) are not, and the anti-aliased edges
    // every glyph here has would come out darkened without this.
    let straight: Vec<u8> = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let demultiplied = pixel.demultiply();
            [
                demultiplied.red(),
                demultiplied.green(),
                demultiplied.blue(),
                demultiplied.alpha(),
            ]
        })
        .collect();

    to_render_image(straight, RENDER_SIZE, RENDER_SIZE)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every slug this module names must actually be a file both
    /// `DefaultIcons` and `LightIcons` embed, and every embedded file must
    /// parse and rasterize — a build-time invariant on the (gitignored,
    /// hand-authored) icon kit, checked here so a bad SVG fails `cargo test`
    /// rather than silently blanking an icon.
    #[test]
    fn every_mapped_slug_rasterizes_in_both_packs() {
        for (class, slug) in CLASS_ICON_SLUGS {
            for pack in [IconPack::Dark, IconPack::Light] {
                assert!(
                    icon_tile(class, pack).is_some(),
                    "{slug}.svg ({class}) failed to rasterize in {pack:?}"
                );
            }
        }
    }

    /// The whole point of `IconPack`: the same class looks up a different
    /// file, and thus different pixels, depending on which pack is asked
    /// for — `part.svg` deliberately uses different fill colours between the
    /// `dark` and `light` folders (see `assets/icons/default/*/part.svg`).
    /// The slug a class draws, or `None` when the kit does not cover it —
    /// the table lookup on its own, without rasterizing anything.
    fn slug_of(class: &str) -> Option<&'static str> {
        CLASS_ICON_SLUGS
            .iter()
            .find(|(name, _)| *name == class)
            .map(|(_, slug)| *slug)
    }

    #[test]
    fn a_class_outside_roblox_metadata_reuses_its_family_tile() {
        // Roblox's own `ExplorerImageIndex` covers none of the left-hand
        // classes, which is why the insert picker used to list them with a
        // bare glyph. Each is pointed at the tile its family already has
        // rather than at a drawing of its own.
        for (outsider, family) in [
            ("FileMesh", "BlockMesh"),
            ("DynamicMesh", "BlockMesh"),
            ("KeyframeSequence", "Animation"),
            ("Vector3Curve", "Animation"),
            ("BinaryStringValue", "StringValue"),
            ("Motor", "Motor6D"),
            ("PartOperation", "UnionOperation"),
            ("VehicleController", "Humanoid"),
        ] {
            let slug = slug_of(outsider).unwrap_or_else(|| panic!("{outsider} has no tile"));
            assert_eq!(
                Some(slug),
                slug_of(family),
                "{outsider} should share {family}'s tile"
            );
        }
    }

    #[test]
    fn a_tile_authored_beyond_the_sheet_is_claimed_by_exactly_its_own_classes() {
        // The five drawings added past Roblox's 147: each exists because no
        // tile in the kit fitted, so each has to be claimed by something,
        // and nothing else may quietly pick it up.
        for (slug, classes) in [
            ("style-sheet", &["StyleBase", "StyleSheet"][..]),
            ("style-rule", &["StyleRule"][..]),
            ("style-link", &["StyleDerive", "StyleLink"][..]),
            ("intersect-operation", &["IntersectOperation"][..]),
            ("body-colors", &["BodyColors"][..]),
        ] {
            let mut claimed: Vec<&str> = CLASS_ICON_SLUGS
                .iter()
                .filter(|(_, s)| *s == slug)
                .map(|(class, _)| *class)
                .collect();
            claimed.sort_unstable();
            assert_eq!(claimed, classes, "{slug}");
        }
    }

    #[test]
    fn a_resolved_tile_is_served_from_the_cache_next_time() {
        // `rasterize` is far too expensive to run once per picker row per
        // frame; the second lookup must hand back the very same image.
        let first = icon_tile("Part", IconPack::Dark).expect("Part is covered by the icon kit");
        let second = icon_tile("Part", IconPack::Dark).expect("Part is covered by the icon kit");
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn clearing_the_cache_forgets_both_variants() {
        let mut cache = TileCache::new();
        cache.insert(IconPack::Dark, "Part", None);
        cache.insert(IconPack::Light, "Part", None);
        assert!(cache.get(IconPack::Dark, "Part").is_some());
        assert!(cache.get(IconPack::Light, "Part").is_some());

        cache.clear();
        assert!(cache.get(IconPack::Dark, "Part").is_none());
        assert!(cache.get(IconPack::Light, "Part").is_none());
    }

    #[test]
    fn a_class_the_kit_does_not_cover_is_remembered_as_a_miss() {
        // The misses are the common case in the picker — every row for a
        // class outside the kit — so re-walking the table for each one is
        // exactly what the cache is there to stop.
        let mut cache = TileCache::new();
        assert!(cache.get(IconPack::Dark, "NotARealClass").is_none());
        cache.insert(IconPack::Dark, "NotARealClass", None);
        assert!(matches!(
            cache.get(IconPack::Dark, "NotARealClass"),
            Some(None)
        ));
    }

    #[test]
    fn icon_tile_reads_from_the_requested_pack() {
        let dark = icon_tile("Part", IconPack::Dark).expect("Part is covered by the icon kit");
        let light = icon_tile("Part", IconPack::Light).expect("Part is covered by the icon kit");
        assert_ne!(dark.as_bytes(0), light.as_bytes(0));
    }

    const RED_SQUARE: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">
        <rect width="24" height="24" fill="#ff0000"/></svg>"##;

    /// The pack is an overlay over the kit: its drawing wins for a class it
    /// names, and every other class is still the kit's.
    #[test]
    fn an_installed_pack_wins_for_the_classes_it_names_and_leaves_the_rest() {
        let overlay = IconOverlay::with("Part", RED_SQUARE);

        let mine = icon_tile_over("Part", IconPack::Dark, Some(&overlay)).unwrap();
        let kit = icon_tile_over("Part", IconPack::Dark, None).unwrap();
        assert_ne!(mine.as_bytes(0), kit.as_bytes(0));

        let untouched = icon_tile_over("Folder", IconPack::Dark, Some(&overlay)).unwrap();
        let folder = icon_tile_over("Folder", IconPack::Dark, None).unwrap();
        assert_eq!(untouched.as_bytes(0), folder.as_bytes(0));
    }

    /// A pack can name a class the kit has no tile for at all, and its icon is
    /// drawn where the kit alone would have fallen back to a glyph.
    #[test]
    fn a_pack_can_cover_a_class_the_kit_does_not() {
        let overlay = IconOverlay::with("NotARealClass", RED_SQUARE);
        assert!(icon_tile_over("NotARealClass", IconPack::Dark, None).is_none());
        assert!(icon_tile_over("NotARealClass", IconPack::Dark, Some(&overlay)).is_some());
    }

    /// A drawing on a 24x24 canvas fills the tile the same way a 16x16 one
    /// does, rather than being cropped to its top-left two thirds: the
    /// far corner pixel of a full-bleed square is opaque either way.
    #[test]
    fn a_larger_canvas_is_scaled_to_fill_the_tile() {
        let overlay = IconOverlay::with("Part", RED_SQUARE);
        let image = icon_tile_over("Part", IconPack::Dark, Some(&overlay)).unwrap();
        let bytes = image.as_bytes(0).unwrap();
        let last_pixel = &bytes[bytes.len() - 4..];
        assert_eq!(last_pixel[3], 255, "bottom-right corner must be opaque");
    }

    /// A 32x16 drawing sits in the middle of the tile, not its top half: the
    /// first row is empty and the middle one is not.
    #[test]
    fn a_drawing_that_is_not_square_is_centred() {
        let wide = br##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="16">
            <rect width="32" height="16" fill="#00ff00"/></svg>"##;
        let overlay = IconOverlay::with("Part", wide);
        let image = icon_tile_over("Part", IconPack::Dark, Some(&overlay)).unwrap();
        let bytes = image.as_bytes(0).unwrap();
        let alpha =
            |row: usize, column: usize| bytes[(row * RENDER_SIZE as usize + column) * 4 + 3];

        assert_eq!(alpha(0, 16), 0, "the top row is padding");
        assert_eq!(alpha(31, 16), 0, "so is the bottom row");
        assert_eq!(alpha(16, 16), 255, "the drawing is in the middle");
    }

    /// A document with no size cannot be scaled to anything; it is refused,
    /// and so falls through to the kit, rather than dividing by zero.
    #[test]
    fn a_drawing_with_no_size_is_refused() {
        let empty = br#"<svg xmlns="http://www.w3.org/2000/svg" width="0" height="0"/>"#;
        assert!(rasterize(empty).is_none());
    }

    /// A pack file that will not parse falls through to the kit rather than
    /// blanking the icon.
    #[test]
    fn a_broken_drawing_falls_back_to_the_kit() {
        let overlay = IconOverlay::with("Part", b"this is not svg");
        let shown = icon_tile_over("Part", IconPack::Dark, Some(&overlay)).unwrap();
        let kit = icon_tile_over("Part", IconPack::Dark, None).unwrap();
        assert_eq!(shown.as_bytes(0), kit.as_bytes(0));
    }
}
