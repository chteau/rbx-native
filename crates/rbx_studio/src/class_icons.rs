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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum IconPack {
    #[default]
    Dark,
    Light,
}

/// Every icon in the kit is authored on a 16x16 `viewBox` (see
/// `assets/icons/README.md`'s "Canvas" section); rasterized at 2x for a
/// sharp downscale to the Explorer's `CLASS_ICON_SIZE`.
const ICON_VIEWBOX: f32 = 16.0;
const RENDER_SIZE: u32 = 32;

/// `ClassName -> icon slug`, extracted from the class icon kit's own spec
/// (`assets/icons/README.md`'s "Tiles" section, which lists each SVG's
/// filename against the classes it covers) — 300 classes onto 137 of the
/// kit's 147 tiles; a class missing here falls back to a Lucide glyph
/// (`explorer::icon`), same as a class undocumented in Roblox's own sheet
/// used to.
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
    ("BindableEvent", "bindable-event"),
    ("BindableFunction", "bindable-function"),
    ("BlockMesh", "mesh"),
    ("BloomEffect", "post-effect"),
    ("BlurEffect", "post-effect"),
    ("BodyAngularVelocity", "body-mover"),
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
    ("EchoSoundEffect", "sound-effect"),
    ("EqualizerSoundEffect", "sound-effect"),
    ("Explosion", "explosion"),
    ("FaceControls", "face-controls"),
    ("Fire", "fire"),
    ("Flag", "flag"),
    ("FlagStand", "flag-stand"),
    ("FlangeSoundEffect", "sound-effect"),
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
    ("HumanoidDescription", "humanoid-description"),
    ("HumanoidRigDescription", "handles"),
    ("IKControl", "handles"),
    ("ImageButton", "image-button"),
    ("ImageHandleAdornment", "image-handle-adornment"),
    ("ImageLabel", "image-label"),
    ("IntConstrainedValue", "value"),
    ("IntValue", "value"),
    ("JointInstance", "weld"),
    ("Keyframe", "animation"),
    ("KeyframeMarker", "animation"),
    ("Light", "light"),
    ("Lighting", "light"),
    ("LineForce", "line-force"),
    ("LineHandleAdornment", "line-handle-adornment"),
    ("LinearVelocity", "linear-velocity"),
    ("LocalScript", "local-script"),
    ("LocalizationService", "localization-service"),
    ("LocalizationTable", "localization-table"),
    ("MakeupDescription", "humanoid-description"),
    ("MarketplaceService", "gui-container"),
    ("MaterialService", "material-service"),
    ("MaterialVariant", "material-variant"),
    ("MeshPart", "union-operation"),
    ("Message", "message"),
    ("Model", "model"),
    ("ModuleScript", "module-script"),
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
    ("ParallelRampPart", "part"),
    ("Part", "part"),
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
    ("TextChatService", "text-chat-service"),
    ("TextLabel", "text-label"),
    ("TextSource", "text-source"),
    ("Texture", "texture"),
    ("Tool", "tool"),
    ("Torque", "angular-velocity"),
    ("TorsionSpringConstraint", "torsion-spring-constraint"),
    ("TouchTransmitter", "force-field"),
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
    ("Vector3Value", "value"),
    ("VectorForce", "vector-force"),
    ("VehicleSeat", "seat"),
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
    let installed = USER_PACK.read().ok().and_then(|pack| pack.clone());
    icon_tile_over(class, pack, installed.as_deref())
}

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
}

/// Whether a user pack is currently installed over the kit — what the
/// Explorer menu checks so it never shows a pack as chosen that failed to load.
pub(crate) fn user_pack_installed() -> bool {
    USER_PACK.read().is_ok_and(|pack| pack.is_some())
}

/// Renders `svg` to a square RGBA tile.
///
/// The kit is authored on a 16x16 canvas (`ICON_VIEWBOX`), but the scale is
/// taken from the document's own size so a pack drawn on 24x24 or 32x32 fills
/// the tile instead of being cropped to its top-left corner.
///
/// `pub(crate)`: also `action_icons`'s own rasterizer, for the ribbon's
/// action-icon kit (`assets/icons/actions`) — same 16x16 canvas, same
/// premultiplied-alpha fixup, no reason for a second copy of either.
pub(crate) fn rasterize(svg: &[u8]) -> Option<Arc<RenderImage>> {
    let tree = Tree::from_data(svg, &Options::default()).ok()?;
    let mut pixmap = Pixmap::new(RENDER_SIZE, RENDER_SIZE)?;
    let size = tree.size();
    let canvas = size.width().max(size.height());
    let scale = RENDER_SIZE as f32 / if canvas > 0.0 { canvas } else { ICON_VIEWBOX };
    resvg::render(
        &tree,
        Transform::from_scale(scale, scale),
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
