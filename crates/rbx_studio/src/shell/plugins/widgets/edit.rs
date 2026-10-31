//! Typing in a widget's `TextBox`: an input field set over the box in the
//! box's own face, size, colours and alignment, while the widget is drawn
//! with the box's text hidden, so the text shows once and focusing the box
//! moves nothing.

use std::borrow::Cow;
use std::collections::HashMap;

use gpui_kit::base::input::{InputEditorStyle, Position};
use gpui_kit::base::Input as TextField;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_lua::plugin::{GuiEventKind, UiId};

use super::Editing;
use crate::shell::Shell;

/// Studio's selection behind selected `TextBox` text, measured off a capture.
const SELECTION: u32 = 0x6ba1f9;

/// What a `TextBox` says about editing it, as last rendered.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::shell::plugins) struct TextBox {
    text: String,
    placeholder: String,
    /// `ClearTextOnFocus`, `true` unless set otherwise, as in Roblox.
    clear_on_focus: bool,
    /// `TextColor3` with `1 - TextTransparency`.
    color: Hsla,
    placeholder_color: Hsla,
}

fn color3(dom_value: Option<&Variant>, fallback: [f32; 3], alpha: f32) -> Hsla {
    let [r, g, b] = match dom_value {
        Some(Variant::Color3(c)) => [c.r, c.g, c.b],
        _ => fallback,
    };
    Rgba { r, g, b, a: alpha }.into()
}

/// Every `TextBox` in a widget's tree.
pub(super) fn text_boxes(dom: &WeakDom) -> HashMap<Ref, TextBox> {
    let mut found = HashMap::new();
    let mut stack: Vec<Ref> = dom.root_refs().to_vec();
    while let Some(referent) = stack.pop() {
        let Some(instance) = dom.get(referent) else {
            continue;
        };
        if instance.class() == "TextBox" {
            let props = instance.properties();
            let string = |name: &str| match props.get(name) {
                Some(Variant::String(text)) => text.clone(),
                _ => String::new(),
            };
            let transparency = match props.get("TextTransparency") {
                Some(Variant::Float32(t)) => *t,
                _ => 0.0,
            };
            let alpha = (1.0 - transparency).clamp(0.0, 1.0);
            found.insert(
                referent,
                TextBox {
                    text: string("Text"),
                    placeholder: string("PlaceholderText"),
                    clear_on_focus: !matches!(
                        props.get("ClearTextOnFocus"),
                        Some(Variant::Bool(false))
                    ),
                    // Roblox's defaults: TextColor3 27, 42, 53 and
                    // PlaceholderColor3 178, 178, 178.
                    color: color3(props.get("TextColor3"), [0.106, 0.165, 0.208], alpha),
                    placeholder_color: color3(props.get("PlaceholderColor3"), [0.7; 3], alpha),
                },
            );
        }
        stack.extend_from_slice(instance.children());
    }
    found
}

/// The widget's tree as drawn while `referent` is typed in: its text and
/// placeholder transparent, so only the field over it shows them, and its
/// box, `AutomaticSize` and all, laid out exactly as before.
pub(super) fn hide_text(dom: &mut WeakDom, referent: Ref) {
    for name in ["TextTransparency", "TextStrokeTransparency"] {
        let _ = dom.set_property(referent, name, Variant::Float32(1.0));
    }
}

impl Shell {
    pub(in crate::shell::plugins) fn begin_widget_edit(
        &mut self,
        id: UiId,
        referent: Ref,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(editing) = &self.plugins.widgets.editing {
            if editing.referent == referent {
                return;
            }
            let previous = editing.referent;
            self.end_widget_edit(previous, false, cx);
        }
        let Some(text_box) = self
            .plugins
            .widgets
            .text_boxes
            .get(&id)
            .and_then(|boxes| boxes.get(&referent))
            .cloned()
        else {
            return;
        };
        let cleared = text_box.clear_on_focus && !text_box.text.is_empty();
        let text = match cleared {
            true => String::new(),
            false => text_box.text.clone(),
        };
        let field = cx.new(|cx| {
            let mut state = InputState::new(window, cx)
                .default_value(text)
                .placeholder(text_box.placeholder.clone());
            state.set_editor_style(InputEditorStyle {
                foreground: text_box.color,
                muted_foreground: text_box.placeholder_color,
                caret: text_box.color,
                selection: rgb(SELECTION).into(),
                ..Default::default()
            });
            state.set_caret_shape(px(1.), 1.0, cx);
            state
        });
        let events = cx.subscribe(
            &field,
            move |shell, field, event: &InputEvent, cx| match event {
                InputEvent::Change => {
                    let text = field.read(cx).value().to_string();
                    shell.send_gui(referent, GuiEventKind::TextChanged(text), cx);
                }
                InputEvent::PressEnter { .. } => shell.end_widget_edit(referent, true, cx),
                InputEvent::Blur => shell.end_widget_edit(referent, false, cx),
                _ => {}
            },
        );
        self.plugins.widgets.editing = Some(Editing {
            widget: id,
            referent,
            field,
            aligned: None,
            unfocused: true,
            _events: events,
        });
        // The next frame asks for the widget again with the box's text
        // hidden (see `hide_text`).
        if cleared {
            self.send_gui(referent, GuiEventKind::TextChanged(String::new()), cx);
        }
        self.send_gui(referent, GuiEventKind::Focused, cx);
        cx.notify();
    }

    pub(in crate::shell::plugins) fn end_widget_edit(
        &mut self,
        referent: Ref,
        enter: bool,
        cx: &mut Context<Self>,
    ) {
        if self
            .plugins
            .widgets
            .editing
            .take_if(|e| e.referent == referent)
            .is_none()
        {
            return;
        }
        self.send_gui(referent, GuiEventKind::FocusLost { enter }, cx);
        cx.notify();
    }

    /// The field over the `TextBox` being typed in on widget `id`, once a
    /// render with its text hidden has said how the box sets its text.
    pub(super) fn widget_edit_field(
        &mut self,
        id: UiId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let widgets = &mut self.plugins.widgets;
        let editing = widgets.editing.as_mut().filter(|e| e.widget == id)?;
        let look = widgets
            .drawn
            .get(&id)?
            .look
            .as_ref()
            .filter(|(at, _)| *at == editing.referent)
            .map(|(_, look)| look.clone())?;
        if let Some(face) = look.face.as_ref() {
            let key = (look.family.clone(), look.weight, look.italic);
            if widgets.fonts.insert(key) {
                if let Err(err) = cx.text_system().add_fonts(vec![Cow::Owned(face.to_vec())]) {
                    eprintln!("rbxstudio: plugin widget font {}: {err}", look.family);
                }
            }
        }
        let align = match look.align[0] {
            a if a < 0.25 => TextAlign::Left,
            a if a > 0.75 => TextAlign::Right,
            _ => TextAlign::Center,
        };
        if editing.aligned != Some(align) {
            editing.aligned = Some(align);
            editing
                .field
                .update(cx, |state, cx| state.set_text_align(align, cx));
        }
        if std::mem::take(&mut editing.unfocused) {
            let field = editing.field.clone();
            window.on_next_frame(move |window, cx| {
                // The caret starts after the text, and focusing comes with it.
                field.update(cx, |state, cx| {
                    state.set_cursor_position(Position::new(0, u32::MAX), window, cx);
                });
            });
            window.refresh();
        }
        let [x, y, width, height] = look.area;
        let top = y + (height - look.line) * look.align[1];
        let mut field = div()
            .absolute()
            .left(px(x))
            .top(px(top))
            .w(px(width))
            .h(px(look.line))
            .text_size(px(look.em))
            .line_height(px(look.line))
            .font_weight(FontWeight(f32::from(look.weight)))
            .child(TextField::new(&editing.field));
        if !look.family.is_empty() {
            field = field.font_family(look.family);
        }
        if look.italic {
            field = field.italic();
        }
        Some(field)
    }
}

/// Paints its child with grayscale text: the widget image beside it is
/// grayscale, as Studio's text is, and subpixel fringes would show the seam.
pub(super) struct Grayscale(pub(super) AnyElement);

impl IntoElement for Grayscale {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Grayscale {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.0.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.0.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let mode = cx.text_rendering_mode();
        cx.set_text_rendering_mode(TextRenderingMode::Grayscale);
        self.0.paint(window, cx);
        cx.set_text_rendering_mode(mode);
    }
}
