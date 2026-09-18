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
//! [`icon_tile`] reads from — no rebuild needed to switch. Swapping in a
//! different icon pack entirely is still on `ROADMAP.md` under "Icon and
//! theme packs" — not implemented yet.

use std::sync::Arc;

use gpui_kit::RenderImage;
use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg::{Options, Tree};

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
    let slug = CLASS_ICON_SLUGS.iter().find(|(name, _)| *name == class)?.1;
    let file = match pack {
        IconPack::Dark => DefaultIcons::get(&format!("{slug}.svg")),
        IconPack::Light => LightIcons::get(&format!("{slug}.svg")),
    }?;
    rasterize(&file.data)
}

/// Renders `svg` (a 16x16-`viewBox` document) to a square RGBA tile.
fn rasterize(svg: &[u8]) -> Option<Arc<RenderImage>> {
    let tree = Tree::from_data(svg, &Options::default()).ok()?;
    let mut pixmap = Pixmap::new(RENDER_SIZE, RENDER_SIZE)?;
    let scale = RENDER_SIZE as f32 / ICON_VIEWBOX;
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
}
