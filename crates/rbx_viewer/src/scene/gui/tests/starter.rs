//! `StarterGui.ShowDevelopmentGui`: the service's own switch for whether
//! anything under it is drawn — see [`super::super::plan::starter`].

use super::*;

/// `StarterGui -> ScreenGui -> Frame`, with the service's flag as given and
/// absent for `None`.
fn starter_gui(show: Option<bool>) -> (WeakDom, Ref) {
    let mut dom = WeakDom::new();
    let starter = dom.new_instance("StarterGui", "StarterGui", None);
    if let Some(show) = show {
        dom.set_property(starter, "ShowDevelopmentGui", Variant::Bool(show))
            .unwrap();
    }
    let gui = dom.new_instance("ScreenGui", "ScreenGui", Some(starter));
    dom.set_property(gui, "ScreenInsets", Variant::Enum(0))
        .unwrap();
    frame(
        &mut dom,
        gui,
        udim2(0.0, 10, 0.0, 20),
        udim2(0.0, 30, 0.0, 40),
    );
    (dom, starter)
}

const FRAME: Rect = Rect {
    x: 10.0,
    y: 20.0,
    width: 30.0,
    height: 40.0,
};

#[test]
fn a_starter_gui_hiding_its_contents_plans_no_screen() {
    let (dom, _) = starter_gui(Some(false));
    assert!(screens(&dom).is_empty());
}

#[test]
fn a_starter_gui_showing_its_contents_plans_them() {
    // Absent, the property is `true`: only a saved place carries it at all.
    for show in [Some(true), None] {
        let (dom, _) = starter_gui(show);
        assert_eq!(rects(&dom), [FRAME], "ShowDevelopmentGui = {show:?}");
    }
}

/// The flag is the service's, not a global one: a `ScreenGui` that lives
/// anywhere else is drawn whatever `StarterGui` says.
#[test]
fn a_screen_outside_the_starter_gui_is_unaffected() {
    let (mut dom, _) = starter_gui(Some(false));
    let part = dom.new_instance("Part", "Part", None);
    let gui = dom.new_instance("ScreenGui", "ScreenGui", Some(part));
    dom.set_property(gui, "ScreenInsets", Variant::Enum(0))
        .unwrap();
    frame(
        &mut dom,
        gui,
        udim2(0.0, 10, 0.0, 20),
        udim2(0.0, 30, 0.0, 40),
    );

    assert_eq!(rects(&dom), [FRAME]);
}

/// `rbxview --show-development-gui`.
#[test]
fn the_command_line_override_puts_the_hidden_screen_back() {
    let (mut dom, _) = starter_gui(Some(false));
    crate::load::show_development_gui(&mut dom);

    assert_eq!(rects(&dom), [FRAME]);
}
