//! GUI at run time: StarterGui into PlayerGui, layout, buttons and text
//! boxes taking the mouse and keyboard before the game.

use rbx_dom::{Variant, WeakDom};
use rbx_sim::{conformance, Input, Session};

/// A bare place with a ScreenGui in StarterGui holding a 200x80 button at
/// (100, 100) and a text box at (100, 250), and `source` as a LocalScript
/// in the ScreenGui.
fn place(source: &str) -> WeakDom {
    let mut dom = conformance::bare_place();
    let starter = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == "StarterGui"))
        .unwrap_or_else(|| dom.new_instance("StarterGui", "StarterGui", None));
    let screen = dom.new_instance("ScreenGui", "Screen", Some(starter));
    let _ = dom.set_property(screen, "IgnoreGuiInset", Variant::Bool(true));
    let udim2 = |x: i32, y: i32| {
        Variant::UDim2(rbx_dom::UDim2 {
            x: rbx_dom::UDim {
                scale: 0.0,
                offset: x,
            },
            y: rbx_dom::UDim {
                scale: 0.0,
                offset: y,
            },
        })
    };
    let button = dom.new_instance("TextButton", "Button", Some(screen));
    let _ = dom.set_property(button, "Position", udim2(100, 100));
    let _ = dom.set_property(button, "Size", udim2(200, 80));
    let text_box = dom.new_instance("TextBox", "Box", Some(screen));
    let _ = dom.set_property(text_box, "Position", udim2(100, 250));
    let _ = dom.set_property(text_box, "Size", udim2(200, 50));
    let script = dom.new_instance("LocalScript", "Probe", Some(screen));
    let _ = dom.set_property(script, "Source", Variant::String(source.into()));
    dom.take_changes();
    dom
}

fn playing(source: &str) -> Session {
    let mut session = Session::new(place(source)).expect("the place runs");
    session.start();
    session.join("Player1", 1, None).expect("joins");
    session.run_for(1.0);
    session
}

fn click(session: &mut Session, x: f32, y: f32) {
    for input in [
        Input::Move {
            position: [x, y],
            delta: [0.0, 0.0],
        },
        Input::Button {
            button: 1,
            down: true,
        },
        Input::Button {
            button: 1,
            down: false,
        },
    ] {
        session.input(0, input);
        session.step();
    }
}

fn key(session: &mut Session, code: &str) {
    for down in [true, false] {
        session.input(
            0,
            Input::Key {
                code: code.into(),
                down,
            },
        );
        session.step();
    }
}

fn output(session: &mut Session) -> Vec<String> {
    session.clients[0].sim.take_output().lines().to_vec()
}

const WATCH: &str = r#"
local UserInputService = game:GetService("UserInputService")
local screen = script.Parent
local function name(input)
    if input.KeyCode ~= Enum.KeyCode.Unknown then
        return input.KeyCode.Name
    end
    return input.UserInputType.Name
end
task.wait()
print("pos", screen.Button.AbsolutePosition, screen.Button.AbsoluteSize)
screen.Button.MouseButton1Down:Connect(function() print("down") end)
screen.Button.Activated:Connect(function(input, n) print("activated", name(input), n) end)
screen.Button.InputBegan:Connect(function(input) print("button.began", name(input)) end)
UserInputService.InputBegan:Connect(function(input, processed)
    print("uis.began", name(input), processed)
end)
screen.Box.FocusLost:Connect(function(enter, input)
    print("lost", enter, input and name(input), screen.Box.Text)
end)
"#;

#[test]
fn a_click_on_a_button_is_the_gui_s_and_typing_goes_into_a_box() {
    let mut session = playing(WATCH);
    let first = output(&mut session);
    assert!(
        first.iter().any(|l| l.contains("pos 100, 42 200, 80")),
        "{first:?}"
    );
    click(&mut session, 200.0, 140.0);
    let lines = output(&mut session);
    let expected = [
        "down",
        "button.began MouseButton1",
        "uis.began MouseButton1 true",
        "activated MouseButton1 0",
    ];
    for want in expected {
        assert!(lines.iter().any(|l| l.ends_with(want)), "{want}: {lines:?}");
    }
    click(&mut session, 200.0, 275.0);
    key(&mut session, "H");
    key(&mut session, "I");
    key(&mut session, "Return");
    let lines = output(&mut session);
    assert!(
        lines.iter().any(|l| l.ends_with("uis.began H true")),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.ends_with("lost true Return hi")),
        "{lines:?}"
    );
}

const PAINT: &str = r#"
local image = game:GetService("AssetService"):CreateEditableImage({ Size = Vector2.new(4, 4) })
local label = Instance.new("ImageLabel")
label.Name = "Canvas"
label.ImageContent = Content.fromObject(image)
label.Parent = script.Parent
image:DrawRectangle(Vector2.zero, image.Size, Color3.new(1, 0, 0), 0, Enum.ImageCombineType.Overwrite)
task.wait(2)
image:DrawRectangle(Vector2.zero, Vector2.new(2, 2), Color3.new(0, 0, 1), 0, Enum.ImageCombineType.Overwrite)
"#;

/// The pixels the viewer draws the `Canvas` label with: the
/// `Content::Object` its `ImageContent` holds, looked up in
/// `rbx_dom::editable`.
fn canvas(session: &Session) -> rbx_dom::editable::Pixels {
    let dom = session.clients[0].sim.dom();
    let mut stack = dom.root_refs().to_vec();
    while let Some(at) = stack.pop() {
        let instance = dom.get(at).expect("listed");
        if instance.name() == "Canvas" {
            let Some(Variant::Content(rbx_dom::Content::Object(object))) =
                instance.properties().get("ImageContent")
            else {
                panic!("ImageContent holds the image");
            };
            let Some(rbx_dom::editable::Editable::Image(pixels)) = rbx_dom::editable::get(*object)
            else {
                panic!("the image is published");
            };
            let pixels = rbx_dom::editable::lock(&pixels).clone();
            return pixels;
        }
        stack.extend(instance.children());
    }
    panic!("the label is in the player's GUI");
}

#[test]
fn an_editable_image_a_script_paints_reaches_the_label_it_is_bound_to() {
    let mut session = playing(PAINT);
    let red = canvas(&session);
    assert_eq!((red.width, red.height), (4, 4));
    assert_eq!(&red.rgba[..4], &[255, 0, 0, 255]);
    session.run_for(2.0);
    let painted = canvas(&session);
    assert!(painted.version > red.version);
    assert_eq!(&painted.rgba[..4], &[0, 0, 255, 255]);
    assert_eq!(&painted.rgba[60..], &[255, 0, 0, 255]);
}
