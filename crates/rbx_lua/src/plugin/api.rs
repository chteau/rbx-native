//! What the editor sees of the plugin host: the inputs it feeds in, the
//! outcome of each dispatch, and the UI the plugins asked for.

use std::path::PathBuf;

use rbx_dom::{Change, Ref, WeakDom};

/// One loaded plugin, stable for the host's lifetime.
pub type PluginId = u32;
/// A toolbar button, plugin action, widget, prompt or menu: ids share one
/// counter, so no two of anything are ever equal.
pub type UiId = u32;

/// Where a plugin came from and what it is made of.
pub struct PluginSource {
    /// Shown in the toolbar tooltip, the Output and the plugin manager;
    /// also the key `plugin:GetSetting` stores under.
    pub name: String,
    pub body: SourceBody,
    pub origin: Origin,
}

pub enum SourceBody {
    /// A lone `.lua`/`.luau` file: run as one `Script` named after the file.
    Luau(String),
    /// A parsed `.rbxm`/`.rbxmx`: every enabled `Script` in it runs
    /// (`LocalScript`s and `ModuleScript`s do not), and `require` resolves
    /// `ModuleScript`s inside it.
    Model(WeakDom),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Origin {
    /// A real Roblox Studio plugin (the user's Studio `Plugins` folder or
    /// rbx-native's own `plugins/roblox` folder).
    Roblox { path: PathBuf },
    /// One of rbx-native's own plugins, described by a `plugin.toml`.
    Native { path: PathBuf, manifest: Manifest },
}

/// A native plugin's `plugin.toml`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub description: String,
    /// Relative to the manifest: an `init.luau` or a `.wasm`/`.wat` module.
    pub entry: String,
    pub permissions: Vec<Permission>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Permission {
    /// Read the place (`dom.read`).
    DomRead,
    /// Edit the place (`dom.write`).
    DomWrite,
    /// Read and change the selection (`selection`).
    Selection,
    /// HTTP to one host (`http:<host>`), `*` for any.
    Http(String),
    /// Breakpoints, watches, stepping (`debugger`).
    Debugger,
}

/// A value handed to (or from) Luau without a VM at hand.
#[derive(Clone, Debug, PartialEq)]
pub enum Arg {
    Nil,
    Bool(bool),
    Number(f64),
    String(String),
    Instance(Ref),
    /// A Luau array of instances.
    Instances(Vec<Ref>),
    Vector2([f32; 2]),
    Vector3([f32; 3]),
    Color3([f32; 3]),
    BrickColor(u32),
    /// `Enum.<enum_name>.<item>`.
    Enum { enum_name: String, item: String },
    /// A Luau dictionary.
    Table(Vec<(String, Arg)>),
    /// A Luau array.
    List(Vec<Arg>),
}

/// Everything the editor knows that plugins can read, refreshed before
/// every dispatch.
#[derive(Clone, Debug, Default)]
pub struct Context {
    /// Seconds on a monotonic clock (only differences matter).
    pub now: f64,
    pub selection: Vec<Ref>,
    /// The debugger as the editor sees it (for `GetDebuggerManager`).
    pub debugger: DebuggerView,
    /// The 3D view's mouse, for `plugin:GetMouse()`: the instance under the
    /// cursor and the hit point, if any.
    pub mouse_target: Option<Ref>,
    pub mouse_hit: Option<[f32; 3]>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DebuggerView {
    /// Every breakpoint: script, 1-based line, condition.
    pub breakpoints: Vec<(Ref, u32, Option<String>)>,
    /// Watch expressions with their last value (`None` = not evaluated).
    pub watches: Vec<(String, Option<String>)>,
    /// Some while a run is paused: script, line, call stack (function, line).
    pub paused: Option<(Ref, u32, Vec<(String, u32)>)>,
    /// A run is in progress (paused or not).
    pub running: bool,
}

/// One thing that happened in the editor, for the plugins to react to.
#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    /// Run every plugin not yet started (after [`PluginHost::add`]).
    Start,
    /// Timers, deferred threads, finished HTTP requests, `Heartbeat`.
    Tick,
    ButtonClicked(UiId),
    ActionTriggered(UiId),
    /// `Context::selection` differs from the last dispatch's.
    SelectionChanged,
    /// The editor itself (Explorer, Properties, a drag, undo…) changed the
    /// place: fires `Changed`, `ChildAdded`, … connections.
    DomChanged(Vec<Change>),
    PromptAnswered(UiId, Answer),
    Gui(GuiEvent),
    Handle(HandleEvent),
    /// The user closed a widget's panel: `Enabled` goes false, `BindToClose`
    /// runs.
    WidgetClosed(UiId),
    /// The debugger paused, resumed or its breakpoints/watches changed.
    DebuggerChanged,
    /// Fires `plugin.Unloading`, then drops the plugin's VM and instances.
    Unload(PluginId),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    /// `None` = cancelled.
    Color3(Option<[f32; 3]>),
    BrickColor(Option<u32>),
    /// An HTTP permission prompt: allowed or not (remembered either way).
    Allow(bool),
    /// A `PluginMenu:ShowAsync`: the chosen action, `None` = dismissed.
    Menu(Option<UiId>),
}

/// Input on a widget's GUI, hit-tested by the editor on its own render.
#[derive(Clone, Debug, PartialEq)]
pub struct GuiEvent {
    pub referent: Ref,
    pub kind: GuiEventKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum GuiEventKind {
    /// Pointer position in widget pixels.
    Enter { x: f32, y: f32 },
    Leave { x: f32, y: f32 },
    Move { x: f32, y: f32 },
    /// `button`: 1 left, 2 right.
    Down { button: u8, x: f32, y: f32 },
    Up { button: u8, x: f32, y: f32 },
    /// A full press and release on the same element: `MouseButton1Click`
    /// (+ `Activated`) on a `GuiButton`.
    Click { button: u8, x: f32, y: f32 },
    /// A `TextBox` gained focus.
    Focused,
    /// A `TextBox`'s text was edited (sets `Text`, fires its change signals).
    TextChanged(String),
    /// A `TextBox` lost focus: `enter` when Return committed it.
    FocusLost { enter: bool },
    /// Mouse wheel over a `ScrollingFrame` (pixels, positive = down).
    Scroll { dy: f32 },
}

/// Input on a plugin's `Handles`/`ArcHandles`/`*HandleAdornment`, picked by
/// the editor in the 3D view.
#[derive(Clone, Debug, PartialEq)]
pub struct HandleEvent {
    pub referent: Ref,
    pub kind: HandleEventKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum HandleEventKind {
    /// `face`: `Enum.NormalId` value (Right 0, Top 1, Back 2, Left 3,
    /// Bottom 4, Front 5).
    Down { face: u8 },
    /// `Handles.MouseDrag(face, distance)` in studs from the grab.
    Drag { face: u8, distance: f32 },
    Up { face: u8 },
    Enter { face: u8 },
    Leave { face: u8 },
    /// `ArcHandles` — `axis`: `Enum.Axis` value (X 0, Y 1, Z 2).
    ArcDown { axis: u8 },
    /// `ArcHandles.MouseDrag(axis, relativeAngle, deltaRadius)`, radians.
    ArcDrag { axis: u8, angle: f32, delta_radius: f32 },
    ArcUp { axis: u8 },
    ArcEnter { axis: u8 },
    ArcLeave { axis: u8 },
    /// A `HandleAdornment`'s own `MouseButton1Down`/`Up`/`MouseEnter`/`Leave`.
    AdornmentDown,
    AdornmentUp,
    AdornmentEnter,
    AdornmentLeave,
}

/// What one dispatch did, for the editor to apply.
#[derive(Debug, Default)]
pub struct Outcome {
    /// The place's change log as the editor should see it: the host's own
    /// instances filtered out, an instance a plugin parked (parented to
    /// `nil`) reported `Removed`, one it brought back reported `Added`.
    pub changes: Vec<Change>,
    /// Some when a plugin set the selection.
    pub selection: Option<Vec<Ref>>,
    pub output: Vec<OutputLine>,
    pub requests: Vec<Request>,
    /// Toolbars, buttons, widgets or a widget's GUI changed: re-read
    /// [`PluginHost::ui`] (and re-render widgets).
    pub ui_changed: bool,
    /// The plugins' `CoreGui`/widget adornments (`Handles`, `ArcHandles`,
    /// `*HandleAdornment`, `SelectionBox`…) changed: re-read
    /// [`PluginHost::adornments`].
    pub adornments_changed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OutputLine {
    /// The plugin's name.
    pub source: String,
    pub kind: OutputKind,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputKind {
    Print,
    Info,
    Warn,
    Error,
}

/// Something only the editor can do.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Undo,
    Redo,
    /// `ChangeHistoryService:SetWaypoint` — the edits so far are one step.
    Waypoint(String),
    /// Ask the user; answer with [`Input::PromptAnswered`].
    Prompt { id: UiId, plugin: PluginId, kind: PromptKind },
    /// `plugin:OpenScript(script, line)`.
    OpenScript { script: Ref, line: Option<u32> },
    Debugger(DebuggerRequest),
    /// `rbxnative.notify(text)` — a toast.
    Notify { plugin: PluginId, text: String },
}

#[derive(Clone, Debug, PartialEq)]
pub enum PromptKind {
    Color3 { initial: Option<[f32; 3]> },
    BrickColor { initial: Option<u32> },
    /// First request from this plugin to this host.
    HttpPermission { host: String },
    /// A `PluginMenu:ShowAsync` at the cursor.
    Menu { title: String, items: Vec<MenuItem> },
}

#[derive(Clone, Debug, PartialEq)]
pub enum MenuItem {
    Action { id: UiId, text: String, icon: String },
    Separator,
    Submenu { title: String, items: Vec<MenuItem> },
}

#[derive(Clone, Debug, PartialEq)]
pub enum DebuggerRequest {
    SetBreakpoint { script: Ref, line: u32, condition: Option<String> },
    RemoveBreakpoint { script: Ref, line: u32 },
    AddWatch(String),
    RemoveWatch(String),
    /// Start debugging this script (opens it first).
    Run { script: Ref },
    Resume,
    StepIn,
    StepOver,
    StepOut,
    Stop,
}

/// The UI the plugins asked for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ui {
    pub toolbars: Vec<Toolbar>,
    pub actions: Vec<Action>,
    pub widgets: Vec<Widget>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Toolbar {
    pub id: UiId,
    pub plugin: PluginId,
    pub name: String,
    pub buttons: Vec<Button>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Button {
    pub id: UiId,
    /// `CreateButton`'s first argument.
    pub key: String,
    pub tooltip: String,
    /// `rbxassetid://…`, `rbxasset://…`, a file path, or empty.
    pub icon: String,
    /// `CreateButton`'s fourth argument (or the key when absent).
    pub text: String,
    pub active: bool,
    pub enabled: bool,
    pub clickable_when_viewport_hidden: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Action {
    pub id: UiId,
    pub plugin: PluginId,
    pub key: String,
    pub text: String,
    pub tooltip: String,
    pub icon: String,
    pub allow_binding: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Widget {
    pub id: UiId,
    pub plugin: PluginId,
    /// `CreateDockWidgetPluginGui`'s id argument.
    pub key: String,
    pub title: String,
    pub enabled: bool,
    /// The `DockWidgetPluginGui` (or floating widget) instance: its
    /// `GuiObject` children are what the editor draws.
    pub root: Ref,
    pub kind: WidgetKind,
    /// Bumped whenever anything under `root` changed.
    pub revision: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum WidgetKind {
    Dock {
        /// `Enum.InitialDockState` value: Top 0, Bottom 1, Left 2, Right 3,
        /// Float 4.
        initial: u8,
        float_size: (f32, f32),
        min_size: (f32, f32),
    },
    /// rbx-native's `plugin:CreateFloatingWidget` — not dockable, drawn
    /// above the editor; dismissed by a click outside it when `light_dismiss`.
    Floating {
        /// `None` = at the cursor when it opens.
        position: Option<(f32, f32)>,
        size: (f32, f32),
        light_dismiss: bool,
    },
}

/// One plugin as the plugin manager lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginInfo {
    pub id: PluginId,
    pub name: String,
    pub origin: Origin,
    /// Whether its scripts have run (`Input::Start`).
    pub started: bool,
    /// The last error that stopped one of its scripts from loading.
    pub error: Option<String>,
}
