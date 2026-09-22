//! The editor's design tokens, taken from the `RbxNative - Studio App` and
//! `InputsStyle` frames in the project's own Figma file rather than
//! invented here, with two deliberate overrides where the frames and
//! WCAG 2.1/2.2 disagree (see `TYPE` and `check_off_border` below).
//!
//! **Nothing outside this module may invent a colour, radius or size.** A
//! literal `rgb(0x…)` or `px(13.)` in chrome is a bug: it drifts away from
//! the design the moment the design moves, and it escapes the UI scale.
//!
//! Text colours are white-on-black alphas rather than separate greys,
//! exactly as the frames specify them (`rgba(255,255,255,0.67)` and
//! friends), so changing a surface re-tints every label sitting on it.
//!
//! Three pieces of *runtime* state live here rather than in a settings
//! struct, because the things that read them — a `.focus()` closure, a
//! `text_size()` call inside an element builder — are styling callbacks
//! with no access to an `App`. They are plain atomics with a setter each;
//! `Shell` calls the setters and then `cx.notify()`, which is what makes a
//! change visible.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use gpui_kit::component::Size;
use gpui_kit::{px, rgb, rgba, BoxShadow, FontWeight, Pixels, Rgba};

// --------------------------------------------------------------- surfaces

/// The window's own ground: the title bar and everything behind the docks.
/// The only true black in the design, so that everything sitting on it
/// reads as sitting *on* something.
///
/// The whole ramp is **neutral** grey, not the cool grey it was: a blue
/// cast on a tool whose entire job is showing somebody else's colours puts
/// a thumb on the scale for every material and texture judged against it.
pub(crate) fn black() -> Rgba {
    rgb(0x0A0A0B)
}

/// A dock's body, and the ribbon's category strip. The frame paints both
/// black, which makes three docks and the window behind them one
/// undifferentiated field — you cannot see where the Explorer ends, and
/// the ribbon's tabs read as a gap rather than as a strip. This is the
/// first step off the ground, and the seam that makes a dock a dock.
pub(crate) fn dock() -> Rgba {
    rgb(0x151515)
}

/// Document tabs, the ribbon body, a dock's active tab, and every input —
/// the raised-panel tone.
pub(crate) fn chrome() -> Rgba {
    rgb(0x1D1D1D)
}

/// A **dropdown**, and only a dropdown: one step above the plain value
/// fields around it.
///
/// A select and a text field do different things — one opens, the other
/// takes typing — and on a dense inspector they were indistinguishable
/// until you noticed the chevron. This is the difference, and it is a
/// surface rather than a border so it costs no layout.
pub(crate) fn field_select() -> Rgba {
    rgb(0x232323)
}

/// A ribbon button: above [`field_select`], the way a key sits above its
/// keyboard.
pub(crate) fn tile() -> Rgba {
    rgb(0x282828)
}

/// The File/Edit/View menu strip, between the title bar and the tabs.
pub(crate) fn menu_bar() -> Rgba {
    rgb(0x1D1D1D)
}

/// The accent rule along the top of the open document's tab. This is the
/// cue WCAG 1.4.11 asks for and the wash above cannot give: 5.8:1 against
/// the strip, and a *shape* rather than a tint, so it survives being
/// printed in grey.
pub(crate) fn tab_active_bar() -> Rgba {
    check_on()
}

/// The active ribbon category tab, against the black strip it sits in.
pub(crate) fn ribbon_tab_active() -> Rgba {
    rgba(0xFFFFFF1A)
}

/// Hover on anything not already lit. Deliberately slight: this UI is dark
/// enough that a strong hover reads as a selection instead.
pub(crate) fn hover() -> Rgba {
    rgba(0xFFFFFF0D)
}

/// A selected Explorer row: the accent, weighted to clear 3:1 against the
/// dock it sits on — a selection is a state, and WCAG 1.4.11 does not
/// exempt it — while still leaving its own label at 4.5:1 on top.
///
/// The brief that asked for this palette wants a much softer (~12%) wash
/// for "active/selected" — see [`accent_soft`] — but at 12% an accent this
/// dark barely lifts off a surface this dark: it fails the 3:1 floor a
/// state indicator has to clear on its own, no separate border to help it.
/// So the Explorer's selection keeps the stronger wash the old palette
/// used, just re-hued.
pub(crate) fn selection() -> Rgba {
    rgba(0x6C7FDBB8)
}

// ---------------------------------------------------------------- borders

/// Before a dock's trailing cell, and between ribbon groups. Document tabs
/// no longer draw one between themselves — a floating label with an accent
/// underline needs no divider to tell it from its neighbour.
pub(crate) fn divider() -> Rgba {
    rgba(0xFFFFFF1C)
}

/// The seam between two property rows. Fainter than [`divider`] on purpose:
/// that one separates one region of chrome from the next, while this one
/// runs under every row in a long list — at full strength it stops reading
/// as a separator and starts reading as a grid drawn over the panel.
///
/// Decorative, so WCAG 1.4.11 does not apply: nothing about a row's meaning
/// or state is carried by it, and the rows either side are already told
/// apart by their own content.
pub(crate) fn row_divider() -> Rgba {
    rgba(0xFFFFFF0F)
}

// ------------------------------------------------------------------- text

/// The window title, and a ribbon category tab while it isn't the active
/// one — the active tab's own label switches to [`check_on`] instead, the
/// one place text itself carries the accent. The only fully white text in
/// the design otherwise.
pub(crate) fn text_full() -> Rgba {
    rgba(0xFFFFFFFF)
}

/// A property's value; a dock tab's title.
pub(crate) fn text_strong() -> Rgba {
    rgba(0xFFFFFFD1)
}

/// A document tab's title; a ribbon button's label.
pub(crate) fn text_label() -> Rgba {
    rgba(0xFFFFFFB8)
}

/// A property's name.
pub(crate) fn text_muted() -> Rgba {
    rgba(0xFFFFFFAD)
}

/// A search field's placeholder.
pub(crate) fn text_placeholder() -> Rgba {
    rgba(0xFFFFFF8F)
}

/// A read-only property — `Class Name` and friends, which the frame dims on
/// both sides of the row rather than hiding.
pub(crate) fn text_disabled() -> Rgba {
    rgba(0xFFFFFF57)
}

pub(crate) fn text_error() -> Rgba {
    rgb(0xE06C6C)
}

// --------------------------------------------------------------- controls

/// A ticked checkbox, straight off the `InputsStyle` frame — and, through
/// [`focus_ring`] and [`selection`], the only saturated colour in the whole
/// UI that isn't a transform tool's own.
pub(crate) fn check_on() -> Rgba {
    rgb(0x6C7FDB)
}

/// The accent as a wash: an "active" or "selected" surface that *isn't*
/// carrying the state on its own (a ribbon category tab, an armed tool
/// button, a panel toggle) — paired with [`accent_line`] or a stronger cue
/// elsewhere on the same control, never alone on something WCAG 1.4.11
/// would call a state indicator in its own right (see [`selection`]).
pub(crate) fn accent_soft() -> Rgba {
    Rgba {
        a: 0.12,
        ..check_on()
    }
}

/// The accent as a 1px line — the border half of an active/selected
/// control, next to [`accent_soft`]'s fill.
///
/// The brief this ramp comes from calls for the accent at 55% here, same as
/// its `--accent-line` token. That reads fine on the light chrome it was
/// drawn against; against this app's near-black surfaces a 55% line falls
/// short of the 3:1 a state's own outline has to clear (WCAG 1.4.11), so
/// this sits higher — still visibly a tint rather than the solid accent,
/// but one that survives the surfaces it's actually drawn on.
pub(crate) fn accent_line() -> Rgba {
    Rgba {
        a: 0.8,
        ..check_on()
    }
}

/// An unticked one. The frame gives it [`chrome`] and no border at all,
/// which on a dark dock is a 1.1:1 box — invisible, and a WCAG 1.4.11
/// failure for a control whose whole job is to show a state. So the fill is
/// the frame's and the outline below is this project's.
pub(crate) fn check_off() -> Rgba {
    chrome()
}

/// Over the 3:1 floor on every surface an unticked checkbox can land on —
/// asserted, because it is the only thing distinguishing "off" from
/// "nothing here".
pub(crate) fn check_off_border() -> Rgba {
    rgb(0x8A8A8A)
}

// ----------------------------------------------------------- tool accents

/// One pastel per transform tool, used **only** on that tool's own ribbon
/// button while it is the active tool — never in panel chrome, text or
/// general borders.
///
/// Colour is never the only cue (WCAG 1.4.1): each tool already has its own
/// icon shape, and the active state adds a 1.5px border in the same pastel,
/// so the state survives grayscale and every kind of colour blindness. The
/// pastels sit far above 3:1 on [`tile`], which is the only surface they
/// appear on.
pub(crate) fn tool_select() -> Rgba {
    rgb(0x8FB8FF)
}

pub(crate) fn tool_move() -> Rgba {
    rgb(0x8FE0B0)
}

pub(crate) fn tool_scale() -> Rgba {
    rgb(0xFF9B9B)
}

pub(crate) fn tool_rotate() -> Rgba {
    rgb(0xFFC98F)
}

/// Align's own, for the button beside the four tools.
pub(crate) fn tool_align() -> Rgba {
    rgb(0xC9A8FF)
}

/// The local-orientation toggle's own.
pub(crate) fn tool_local() -> Rgba {
    rgb(0x8FE0D8)
}

/// The Sun tool's own, on whichever of its Sun and Moon tiles is active.
/// Yellower than Rotate's orange, so the two never read as one tool.
pub(crate) fn tool_sun() -> Rgba {
    rgb(0xFFE88F)
}

/// The same pastel as the fill behind an active tool's icon: the frame's
/// own wash weight, low enough that the icon stays the brightest thing in
/// the button.
pub(crate) fn tool_wash(tool: Rgba) -> Rgba {
    Rgba { a: 0.12, ..tool }
}

/// A token, by the name this module calls it.
#[cfg(test)]
pub(crate) type Named = (&'static str, fn() -> Rgba);

/// Every pastel, for the test that checks them all against [`tile`].
#[cfg(test)]
pub(crate) const TOOL_ACCENTS: [Named; 7] = [
    ("select", tool_select),
    ("move", tool_move),
    ("scale", tool_scale),
    ("rotate", tool_rotate),
    ("align", tool_align),
    ("local", tool_local),
    ("sun", tool_sun),
];

// ----------------------------------------------------------------- shapes

/// The border an active transform tool wears, on top of its wash — the
/// non-colour half of WCAG 1.4.1's "never colour alone".
///
/// Scaled, unlike the radii below, precisely *because* it is the
/// accessibility cue: a constant 1.5px would get relatively thinner as the
/// scale grows, i.e. hardest to see exactly when someone has asked for
/// everything to be bigger.
pub(crate) fn tool_border() -> Pixels {
    px((1.5 * font_scale()).max(1.5))
}

/// Everything rounded in this design is rounded by exactly this much:
/// ribbon buttons, dock tabs, inputs, checkboxes, value fields.
pub(crate) const RADIUS: Pixels = px(5.);
/// Except a colour swatch, which is barely rounded at all.
pub(crate) const RADIUS_TINY: Pixels = px(1.);

// -------------------------------------------------------------- UI  scale
//
// Blender's model, and for Blender's reason: one multiplier over fonts
// *and* the boxes they sit in, rather than VS Code's split between UI zoom
// and editor font size. A dense inspector whose text grows but whose rows
// don't is worse than either.
//
// This is how this app meets WCAG 1.4.4 (Resize Text, 200%) — a native app
// has no browser zoom, so the settings-based scale *is* the mechanism.

/// The range the scale may be set to, matching Blender's Resolution Scale.
pub(crate) const FONT_SCALE_RANGE: (f32, f32) = (0.5, 2.0);

static FONT_SCALE: AtomicU32 = AtomicU32::new(1.0f32.to_bits());

pub(crate) fn font_scale() -> f32 {
    f32::from_bits(FONT_SCALE.load(Ordering::Relaxed))
}

/// Returns whether the scale actually changed, so the caller knows whether
/// a re-render is worth asking for.
pub(crate) fn set_font_scale(scale: f32) -> bool {
    let scale = scale.clamp(FONT_SCALE_RANGE.0, FONT_SCALE_RANGE.1);
    f32::from_bits(FONT_SCALE.swap(scale.to_bits(), Ordering::Relaxed)) != scale
}

/// A design value in the scale the frames are drawn at, as the pixels to
/// actually paint. **Every** size in this module goes through here — a
/// literal `px()` outside it is a size the accessibility scale can't reach.
fn scaled(base: f32) -> Pixels {
    px(base * font_scale())
}

/// A width in the frames' own scale, as pixels to paint. Public because a
/// few widths live where they are used rather than here — a dropdown sized
/// to its own longest label, say — and they still have to follow the scale.
pub(crate) fn scaled_width(base: f32) -> Pixels {
    scaled(base)
}

/// WCAG 2.5.8's minimum pointer target, and 2.5.5's enhanced one. Floors,
/// not design values: what the criteria require regardless of what anyone
/// has set the scale to.
const TARGET_FLOOR: f32 = 24.;
const TARGET_FLOOR_LARGE: f32 = 44.;

static LARGE_TARGETS: AtomicBool = AtomicBool::new(false);

/// Whether the enhanced target floor is in force.
pub(crate) fn large_targets() -> bool {
    LARGE_TARGETS.load(Ordering::Relaxed)
}

/// Returns whether the setting actually changed.
pub(crate) fn set_large_targets(large: bool) -> bool {
    LARGE_TARGETS.swap(large, Ordering::Relaxed) != large
}

fn target_floor() -> f32 {
    if large_targets() {
        TARGET_FLOOR_LARGE
    } else {
        TARGET_FLOOR
    }
}

/// [`scaled`] for anything a pointer has to hit, which therefore may grow
/// with the scale but may never shrink below [`TARGET_FLOOR`].
///
/// Scaling *down* is where this matters. At 0.5x a plain `scaled()` row is
/// 14px and a checkbox 13px — comfortably under the floor, in the exact
/// configuration a low-vision user is least likely to be using and a
/// motor-impaired one most. The previous version of this module scaled them
/// freely and asserted the floor only at 1.0x, which is a test that cannot
/// fail.
fn scaled_target(base: f32) -> Pixels {
    px((base * font_scale()).max(target_floor()))
}

// ------------------------------------------------------------------- type
//
// The frames set body text at 9px. That is unreadable at arm's length on a
// 1440p panel and fails the *intent* of WCAG 1.4.4 before the scale is even
// touched, so this is the one place the design is deliberately overruled:
// the ramp below is the frames' hierarchy at a legible base size.

/// The default: property names and values, tree rows, dock tabs, document
/// tab titles, the window title, the menu bar, ribbon category tabs.
pub(crate) fn text_md() -> Pixels {
    scaled(14.)
}

/// A property section's header, and a tooltip.
pub(crate) fn text_sm() -> Pixels {
    scaled(13.)
}

/// A ribbon button's label, where the word has 42px to fit in.
pub(crate) fn text_xs() -> Pixels {
    scaled(11.)
}

pub(crate) fn line_md() -> Pixels {
    scaled(20.)
}

pub(crate) fn line_sm() -> Pixels {
    scaled(18.)
}

pub(crate) fn line_xs() -> Pixels {
    scaled(15.)
}

/// A property category's header, and nothing else.
pub(crate) const WEIGHT_BOLD: FontWeight = FontWeight::BOLD;

/// The design is set in Manrope. `main::install_fonts` only names it when
/// the machine actually has it, so one without falls back to the platform
/// UI font rather than to nothing.
pub(crate) const FONT_FAMILY_UI: &str = "Manrope";
pub(crate) const FONT_FAMILY_MONO: &str = "JetBrains Mono";

/// The size to hand a toolkit widget — an `Input`, a `Select`, a
/// `ColorPicker` — so its text comes out at [`text_md`].
///
/// The toolkit sizes text in fixed steps and scales `Size::Size` by 0.875,
/// so this is [`text_md`] said backwards. Without it the Properties panel
/// reads as two type scales, one for names and a bigger one for values.
pub(crate) fn field_size() -> Size {
    Size::Size(px(f32::from(text_md()) / 0.875))
}

// ------------------------------------------------------------- dimensions
//
// All of these scale with the type, Blender-style: a 2x scale that grew the
// labels but left the rows at 20px would just clip them.

pub(crate) fn topbar_height() -> Pixels {
    scaled(34.)
}

pub(crate) fn menu_bar_height() -> Pixels {
    scaled(30.)
}

pub(crate) fn tabs_height() -> Pixels {
    scaled(42.)
}

pub(crate) fn ribbon_tabs_height() -> Pixels {
    scaled(28.)
}

pub(crate) fn ribbon_height() -> Pixels {
    scaled(80.)
}

pub(crate) fn dock_tabs_height() -> Pixels {
    scaled(38.)
}

/// A document tab's fixed width — tabs don't grow to fit their titles here.
pub(crate) fn tab_width() -> Pixels {
    scaled(180.)
}

/// The "+" cell at the end of a tab strip, and its narrower dock twin. Both
/// clear WCAG 2.5.8's 24x24 floor at every scale ≥ 0.6.
pub(crate) fn tab_add_width() -> Pixels {
    scaled(42.)
}

pub(crate) fn dock_tab_add_width() -> Pixels {
    scaled(36.)
}

/// Each side dock. Wider than the frame's 228 because the label column is
/// wider, because the label is 14px instead of 9px.
pub(crate) fn dock_width() -> f32 {
    300. * font_scale()
}

/// The bottom dock: its tab strip and five rows under it — Output's log, or
/// the Viewport dock's settings.
///
/// Counted in rows rather than scaled from one number, because a row has
/// WCAG's 24px floor under it: at 0.5x the rows barely shrink, and a dock
/// that halved would clip them.
pub(crate) fn dock_height() -> f32 {
    f32::from(dock_tabs_height() + row_height() * 5.)
}

/// A ribbon button, and the stacked-row column beside it.
pub(crate) fn tile_width() -> Pixels {
    scaled(56.)
}

pub(crate) fn stack_width() -> Pixels {
    scaled(100.)
}

pub(crate) fn separator_height() -> Pixels {
    scaled(58.)
}

/// One property row, and its name column. A row is a pointer target (it
/// hovers, and its control lives inside it), so it takes the floor.
pub(crate) fn row_height() -> Pixels {
    scaled_target(28.)
}

/// A property row's name column.
///
/// Sized for the common `BasePart` names (`CollisionGroup`,
/// `MaterialVariant`) at [`text_md`], not for the longest one: every pixel
/// here comes out of the value column on every row, and a panel whose
/// short names sit a long way from their values has to be dragged wider
/// just to read them. The rare long name truncates.
pub(crate) fn row_label_width() -> Pixels {
    scaled(120.)
}

/// A property section's header.
pub(crate) fn section_height() -> Pixels {
    scaled(30.)
}

/// One Explorer row — clickable, so likewise floored.
pub(crate) fn tree_row_height() -> Pixels {
    scaled_target(28.)
}

/// A text field, a select trigger, a stepper, the search box at the top of
/// a dock — all one height, straight off the `InputsStyle` frame, and
/// comfortably over WCAG 2.5.8's 24x24 target floor.
pub(crate) fn input_height() -> Pixels {
    scaled_target(31.)
}

/// The narrowest a single field of a composite value (a `Vector3`'s `X`)
/// may get before it wraps to the next line. Wide enough for a label, a
/// sign and four digits.
pub(crate) fn field_min_width() -> Pixels {
    scaled(76.)
}

/// The top inset a toolkit `Select` needs to sit level with the text
/// fields beside it.
///
/// Measured, not derived: the toolkit's select trigger lays its row out at
/// its own fixed step height and **top-aligns** it inside whatever height
/// it is given, so a select dropped into a 31px field box rides 2-4px high
/// depending on its padding. Its text also sits ~1.5px above its own row's
/// centre. Together that is the 4.5px below — a compensation for somebody
/// else's layout, which is why it is one named value here rather than a
/// bare `px()` at the call site, and why it scales with everything else.
///
/// If a toolkit upgrade changes the select's internals this will be wrong
/// and will need re-measuring against a plain text field in the same panel.
pub(crate) fn select_inset() -> Pixels {
    scaled(4.5)
}

/// The room a toolkit `Select` keeps for its chevron, which it draws at a
/// fixed 15px at an 8px inset (`UX_GUIDELINES.md` §11) whatever the UI
/// scale. **Not** scaled, deliberately: a select sized as one scaled width
/// gives its label less and less room as the scale drops, until at 0.5x
/// "Automatic" no longer fits.
pub(crate) fn select_chevron_room() -> Pixels {
    px(31.)
}

/// `InputsStyle` again: the frame's inputs are padded 8px horizontally.
pub(crate) fn input_padding() -> Pixels {
    scaled(8.)
}

/// The checkbox someone actually sees. 15px — the frame's 26, taken down
/// twice on review, because at anything larger it was the loudest thing in
/// a property row and pulled the eye off the values it sits beside.
///
/// Shrinking the *box* is fine; shrinking the *target* is not, which is
/// what [`checkbox_target`] is for. WCAG 2.5.8 is explicit that the icon
/// may be smaller than the target it sits in.
pub(crate) fn checkbox_size() -> Pixels {
    scaled(15.)
}

/// The square a click has to land in to toggle that checkbox — never under
/// WCAG 2.5.8's floor, whatever the box inside it is doing.
pub(crate) fn checkbox_target() -> Pixels {
    scaled_target(26.)
}

/// The column a property row's expander chevron — and a section header's —
/// sits in, and, because a child field's name lines up under its parent's
/// rather than under the chevron, the step one level of nesting indents by.
/// Exactly the [`text_xs`] icon's own box: the glyph already sits well
/// inside that box, and [`label_gap`] is the air after it, so a wider slot
/// only pushes every name in the panel further from the edge.
pub(crate) fn chevron_slot() -> Pixels {
    scaled(11.)
}

/// The rail a property slider runs its value along. Thick enough to carry
/// the outline an empty one needs (see `shell::rows::slider`), thin enough
/// that the grip still reads as the control.
pub(crate) fn slider_rail() -> Pixels {
    scaled(6.)
}

/// The grip on that rail. Smaller than the strip a click has to land in,
/// the same way [`checkbox_size`] is smaller than [`checkbox_target`].
pub(crate) fn slider_thumb() -> Pixels {
    scaled(13.)
}

/// Below this a rail has fewer pixels than the value has steps, and
/// dragging it stops meaning anything. The row lets it push the panel
/// sideways rather than shrink past it.
pub(crate) fn slider_min_width() -> Pixels {
    scaled(60.)
}

/// The smallest square any icon-only button is allowed to be.
pub(crate) fn hit_target() -> Pixels {
    scaled_target(target_floor())
}

// ---------------------------------------------------- properties  spacing
//
// Gestalt proximity: the gap *between* groups has to be visibly larger than
// the gap *within* one, or the whole panel collapses into a single
// undifferentiated block. These four values are that ratio.

pub(crate) fn panel_padding() -> Pixels {
    scaled(20.)
}

pub(crate) fn row_gap() -> Pixels {
    scaled(8.)
}

/// A property row's inset from the panel's edge, both sides. Small, because
/// every name is indented past a chevron's slot on top of it (see
/// `shell::rows::name_indent`) and the dock already has its own inset.
pub(crate) fn row_padding() -> Pixels {
    scaled(4.)
}

/// Between a category header and its first row.
pub(crate) fn section_gap() -> Pixels {
    scaled(14.)
}

/// Between one category's last row and the next category's header — the
/// largest gap in the panel, and deliberately so.
pub(crate) fn group_gap() -> Pixels {
    scaled(24.)
}

/// Between two category headers with nothing between them. A collapsed
/// category is a tile, not a void: the header carries its own surface (see
/// `shell::rows::section_header`), so the grouping is read off the fill
/// and a full [`group_gap`] of dock between two bars just reads as a hole.
pub(crate) fn header_gap() -> Pixels {
    scaled(4.)
}

/// Inside a row, between a label and its own control. Shorter than
/// [`row_gap`] so each label unambiguously binds to its own input.
pub(crate) fn label_gap() -> Pixels {
    scaled(4.)
}

// -------------------------------------------------------------- behaviour
//
// Nothing below is measurable in a static frame. A design file has no
// keyboard focus, no open menu, no hover and no motion preference, so these
// are this project's own — chosen to sit inside the palette above rather
// than beside it.

/// Keyboard focus, and **only** keyboard focus (WCAG 2.4.7): a solid 2px
/// blue outline, held one pixel clear of the control's own edge.
///
/// Three things about the shape, all of them load-bearing:
///
/// - It is **outset**. WCAG 2.4.13 fails a bare 2px line drawn *inside* a
///   control, because the indicator has to cover at least the area of a 2px
///   perimeter and an inset line eats into the control instead of adding to
///   it.
/// - The 1px surface-coloured step is the gap, which is what keeps the ring
///   legible against a control that happens to be a similar blue.
/// - The order is the whole trick: GPUI paints a shadow list front-to-back
///   in *array* order, so the surface step lands on top and punches the
///   middle out of the blue one. Reverse them and an unfilled control — a
///   ribbon tab, a section header — comes back flooded solid blue.
///
/// [`check_on`] clears 3:1 against every surface in this module (asserted
/// in `tests`), which is WCAG 2.4.13's other half.
///
/// **This paints unconditionally**, and every caller applies it through
/// GPUI's `focus_visible` — its `:focus-visible` equivalent, which already
/// tracks input modality and refreshes the window when it flips. That is
/// what stops a mouse click from lighting up every button the pointer
/// touches, and it is the reason nothing here tracks modality itself: a
/// second source of truth for "was that the keyboard" is a second thing to
/// get wrong. Roving-tabindex children are no exception — each one owns a
/// real focus handle (see `shell::roving`), so `focus_visible` covers them
/// too.
pub(crate) fn focus_ring(surface: Rgba) -> Vec<BoxShadow> {
    vec![ring(check_on(), 3.), ring(surface, 1.)]
}

/// The OS "reduce motion" preference, mirrored from `App::reduce_motion`
/// so that a styling callback can read it.
///
/// GPUI owns the canonical flag — and `with_animation` already honours it —
/// but it never learns the platform's setting on its own (nothing in any of
/// its backends ever calls `set_reduce_motion`), and a `group_hover` style
/// is evaluated with no `App` in reach. So `main` probes the desktop for
/// the real preference once, sets both, and this is the copy the styling
/// side reads. Defaults to *off*.
static REDUCED_MOTION: AtomicBool = AtomicBool::new(false);

pub(crate) fn reduced_motion() -> bool {
    REDUCED_MOTION.load(Ordering::Relaxed)
}

pub(crate) fn set_reduced_motion(reduced: bool) -> bool {
    REDUCED_MOTION.swap(reduced, Ordering::Relaxed) != reduced
}

/// A surface that floats over the shell: a menu, a popover, a tooltip.
/// The frame contains none, and on a UI this dark a drop shadow alone is
/// invisible — so the separation is carried by the hairline, with the
/// shadow only deepening the ground beneath it.
pub(crate) fn elevation() -> Vec<BoxShadow> {
    vec![
        ring(divider(), 1.),
        BoxShadow {
            color: rgba(0x00000099).into(),
            offset: gpui_kit::point(px(0.), px(4.)),
            blur_radius: px(16.),
            spread_radius: px(0.),
            inset: false,
        },
    ]
}

/// [`focus_ring`] for a control that sits flush against its container's
/// edge — a document tab, a ribbon category tab.
///
/// An outset ring on those is clipped to two sides of four, which measures
/// about half the area WCAG 2.4.13 requires (`4w + 4h` for a 2px
/// perimeter). The criterion's own escape is to go *inside* instead, at
/// 3px rather than 2 — so that is what this does, and nothing can clip it.
pub(crate) fn focus_ring_inset() -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: check_on().into(),
        offset: gpui_kit::point(px(0.), px(0.)),
        blur_radius: px(0.),
        spread_radius: px(3.),
        inset: true,
    }]
}

fn ring(color: Rgba, width: f32) -> BoxShadow {
    BoxShadow {
        color: color.into(),
        offset: gpui_kit::point(px(0.), px(0.)),
        blur_radius: px(0.),
        spread_radius: px(width),
        inset: false,
    }
}

/// How long a menu takes to arrive. Short enough to feel like a response,
/// long enough to show where the menu came from.
pub(crate) const DURATION_MENU: Duration = Duration::from_millis(160);

/// Fast out, slow in — the curve everything in this shell animates on.
pub(crate) fn easing_soft(delta: f32) -> f32 {
    let t = delta.clamp(0., 1.);
    1. - (1. - t).powi(3)
}

#[cfg(test)]
#[path = "tokens/tests.rs"]
mod tests;
