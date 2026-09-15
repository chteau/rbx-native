use super::*;

fn parse(args: &[&str]) -> Result<Options, String> {
    Options::parse(args.iter().map(|a| a.to_string()))
}

#[test]
fn a_lone_path_defaults_to_windowed_mode() {
    let options = parse(&["place.rbxl"]).unwrap();

    assert_eq!(options.path(), Path::new("place.rbxl"));
    assert_eq!(options.screenshot(), None);
    assert_eq!(options.size(), DEFAULT_SIZE);
    assert_eq!(options.yaw(), None);
    assert_eq!(options.pitch(), None);
    assert_eq!(options.eye_look_at(), None);
    assert!(options.textures());
    assert!(options.materials());
    assert!(options.lights());
    // Free-flight-at-spawn is the default; --orbit opts back into auto-orbiting.
    assert!(!options.orbit());
    assert_eq!(options.speed(), None);
    assert_eq!(options.sensitivity(), DEFAULT_SENSITIVITY);
    assert_eq!(options.clock_time(), None);
    // The top level by default: a screenshot must not change because a machine
    // is slower than the one the fixture was taken on.
    assert_eq!(options.quality(), QualityLevel::Level(QualityLevel::MAX));
}

#[test]
fn a_quality_level_is_read_by_number_word_or_enum_name() {
    let quality = |text: &str| parse(&["a.rbxl", "--quality", text]).map(|o| o.quality());

    assert_eq!(quality("1"), Ok(QualityLevel::Level(1)));
    assert_eq!(quality("Level07"), Ok(QualityLevel::Level(7)));
    assert_eq!(quality("auto"), Ok(QualityLevel::Automatic));
    assert!(quality("22").is_err());
    assert!(quality("ultra").is_err());
    assert!(parse(&["a.rbxl", "--quality"]).is_err());
}

#[test]
fn a_clock_time_is_read_as_fractional_hours() {
    assert_eq!(
        parse(&["a.rbxl", "--clock-time", "18.3"])
            .unwrap()
            .clock_time(),
        Some(18.3)
    );
    // Midnight is a legitimate hour, and 0 is not "unset".
    assert_eq!(
        parse(&["a.rbxl", "--clock-time", "0"])
            .unwrap()
            .clock_time(),
        Some(0.0)
    );
    assert!(parse(&["a.rbxl", "--clock-time", "dusk"]).is_err());
    assert!(parse(&["a.rbxl", "--clock-time"]).is_err());
}

#[test]
fn orbit_speed_and_sensitivity_are_picked_up() {
    let options = parse(&["--orbit", "--speed", "50", "--sensitivity", "0.5", "m.rbxm"]).unwrap();

    assert!(options.orbit());
    assert_eq!(options.speed(), Some(50.0));
    assert_eq!(options.sensitivity(), 0.5);
}

#[test]
fn speed_and_sensitivity_must_be_positive() {
    assert!(parse(&["a.rbxl", "--speed", "0"]).is_err());
    assert!(parse(&["a.rbxl", "--speed", "-5"]).is_err());
    assert!(parse(&["a.rbxl", "--sensitivity", "nope"]).is_err());
}

#[test]
fn screenshot_and_size_are_picked_up_in_any_order() {
    let options = parse(&["--size", "320x240", "--screenshot", "o.png", "m.rbxm"]).unwrap();

    assert_eq!(options.screenshot(), Some(Path::new("o.png")));
    assert_eq!(options.size(), (320, 240));
    assert_eq!(options.path(), Path::new("m.rbxm"));
}

#[test]
fn title_keeps_only_the_file_name() {
    assert_eq!(
        parse(&["/tmp/places/place.rbxl"]).unwrap().title(),
        "rbxview — place.rbxl"
    );
}

#[test]
fn yaw_pitch_and_no_textures_are_picked_up() {
    let options = parse(&["--yaw", "-135", "--pitch", "-60", "--no-textures", "m.rbxm"]).unwrap();

    assert_eq!(options.yaw(), Some(-135.0));
    assert_eq!(options.pitch(), Some(-60.0));
    assert!(!options.textures());
    // The two downloads are gated separately.
    assert!(options.materials());
}

#[test]
fn materials_can_be_turned_off_on_their_own() {
    let options = parse(&["m.rbxm", "--no-materials"]).unwrap();

    assert!(!options.materials());
    assert!(options.textures());
    assert!(options.lights());
}

#[test]
fn lights_can_be_turned_off_on_their_own() {
    let options = parse(&["m.rbxm", "--no-lights"]).unwrap();

    assert!(!options.lights());
    assert!(options.textures());
    assert!(options.materials());
}

#[test]
fn trails_can_be_turned_off_on_their_own() {
    let options = parse(&["m.rbxm", "--no-trails"]).unwrap();

    assert!(!options.trails());
    assert!(options.beams());
    assert!(options.particles());
    assert!(options.gui());
}

#[test]
fn the_gui_overlay_can_be_turned_off_on_its_own() {
    let options = parse(&["m.rbxm", "--no-gui"]).unwrap();

    assert!(!options.gui());
    assert!(options.trails());
    assert!(options.textures());
}

#[test]
fn malformed_arguments_are_rejected() {
    assert!(parse(&[]).is_err());
    assert!(parse(&["--wat", "a.rbxl"]).is_err());
    assert!(parse(&["--size"]).is_err());
    assert!(parse(&["a.rbxl", "--size", "wide"]).is_err());
    assert!(parse(&["a.rbxl", "--size", "0x240"]).is_err());
    assert!(parse(&["a.rbxl", "b.rbxl"]).is_err());
    assert!(parse(&["a.rbxl", "--yaw", "sideways"]).is_err());
    assert!(parse(&["a.rbxl", "--pitch", "up"]).is_err());
}

#[test]
fn eye_and_look_at_are_picked_up_together() {
    let options = parse(&["--eye", "1,2,3", "--look-at", "-4,5.5,6", "m.rbxm"]).unwrap();

    assert_eq!(
        options.eye_look_at(),
        Some(((1.0, 2.0, 3.0), (-4.0, 5.5, 6.0)))
    );
}

#[test]
fn eye_without_look_at_is_rejected_and_vice_versa() {
    assert!(parse(&["a.rbxl", "--eye", "1,2,3"]).is_err());
    assert!(parse(&["a.rbxl", "--look-at", "1,2,3"]).is_err());
}

#[test]
fn malformed_positions_are_rejected() {
    assert!(parse(&["a.rbxl", "--eye", "1,2", "--look-at", "0,0,0"]).is_err());
    assert!(parse(&["a.rbxl", "--eye", "x,y,z", "--look-at", "0,0,0"]).is_err());
    assert!(parse(&["a.rbxl", "--eye", "1,2,3,4", "--look-at", "0,0,0"]).is_err());
}
