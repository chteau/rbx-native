//! Draft tests: edits forgotten on revert, updates per field, local reads and parses.

use super::*;
use crate::shell::game_settings::catalog::LocalKind;

fn place(name: &str, root: bool) -> PlaceSettings {
    PlaceSettings {
        display_name: name.to_string(),
        description: format!("{name} description"),
        server_size: 50,
        root,
    }
}

fn remote(place_id: u64) -> Remote {
    Remote {
        experience: ExperienceSettings {
            display_name: "Exp".to_string(),
            description: String::new(),
            visibility: Visibility::Public,
            age_rating: Some("13_PLUS".to_string()),
            voice_chat_enabled: false,
            private_server_price_robux: None,
            desktop_enabled: true,
            mobile_enabled: true,
            tablet_enabled: true,
            console_enabled: false,
            vr_enabled: false,
            root_place_id: 1,
        },
        root_place: place("Root", true),
        place: place(if place_id == 1 { "Root" } else { "Other" }, place_id == 1),
        place_id,
    }
}

fn draft(place_id: u64) -> CloudDraft {
    let mut d = CloudDraft::default();
    d.reset(remote(place_id));
    d
}

fn index_of(property: &str) -> usize {
    LOCAL_FIELDS
        .iter()
        .position(|f| f.property == property)
        .unwrap()
}

fn field(property: &str) -> &'static LocalField {
    &LOCAL_FIELDS[index_of(property)]
}

#[test]
fn nothing_before_the_first_load() {
    let mut d = CloudDraft::default();
    assert_eq!(d.remote_value(CloudField::Name), None);
    d.set(CloudField::Desktop, CloudValue::Bool(false));
    assert!(!d.is_dirty());
}

#[test]
fn remote_values_map_each_field() {
    let d = draft(1);
    assert_eq!(
        d.remote_value(CloudField::Name),
        Some(CloudValue::Text("Root".into()))
    );
    assert_eq!(
        d.remote_value(CloudField::ServerSize),
        Some(CloudValue::Count(50))
    );
    assert_eq!(
        d.remote_value(CloudField::PrivateServers),
        Some(CloudValue::Price(None))
    );
    assert_eq!(
        d.remote_value(CloudField::Visibility),
        Some(CloudValue::Fixed("Public".into()))
    );
    assert_eq!(
        d.remote_value(CloudField::AgeRating),
        Some(CloudValue::Fixed("13+".into()))
    );
}

#[test]
fn unrated_and_other_visibility_read_as_words() {
    let mut r = remote(1);
    r.experience.age_rating = None;
    r.experience.visibility = Visibility::Other("FRIENDS".into());
    let mut d = CloudDraft::default();
    d.reset(r);
    assert_eq!(
        d.remote_value(CloudField::AgeRating),
        Some(CloudValue::Fixed("Not rated".into()))
    );
    assert_eq!(
        d.remote_value(CloudField::Visibility),
        Some(CloudValue::Fixed("FRIENDS".into()))
    );
}

#[test]
fn an_edit_put_back_is_forgotten() {
    let mut d = draft(1);
    d.set(CloudField::Console, CloudValue::Bool(true));
    assert!(d.is_edited(CloudField::Console) && d.is_dirty());
    d.set(CloudField::Console, CloudValue::Bool(false));
    assert!(!d.is_edited(CloudField::Console) && !d.is_dirty());
}

#[test]
fn read_only_fields_ignore_edits() {
    let mut d = draft(1);
    d.set(CloudField::Visibility, CloudValue::Fixed("Private".into()));
    d.set(CloudField::AgeRating, CloudValue::Fixed("17+".into()));
    assert!(!d.is_dirty());
}

#[test]
fn reset_drops_edits() {
    let mut d = draft(1);
    d.set(CloudField::Vr, CloudValue::Bool(true));
    d.reset(remote(1));
    assert!(!d.is_dirty());
}

#[test]
fn rebase_keeps_only_edits_that_still_differ() {
    let mut d = draft(1);
    d.set(CloudField::Console, CloudValue::Bool(true));
    d.set(CloudField::Vr, CloudValue::Bool(true));
    let mut landed = remote(1);
    landed.experience.console_enabled = true;
    d.rebase(landed);
    assert!(!d.is_edited(CloudField::Console));
    assert!(d.is_edited(CloudField::Vr));
    assert_eq!(d.value(CloudField::Console), Some(CloudValue::Bool(true)));
}

#[test]
fn value_prefers_the_edit() {
    let mut d = draft(1);
    d.set(CloudField::VoiceChat, CloudValue::Bool(true));
    assert_eq!(d.value(CloudField::VoiceChat), Some(CloudValue::Bool(true)));
    assert_eq!(
        d.remote_value(CloudField::VoiceChat),
        Some(CloudValue::Bool(false))
    );
}

#[test]
fn experience_edits_go_to_the_experience_update() {
    let mut d = draft(1);
    d.set(CloudField::Console, CloudValue::Bool(true));
    d.set(CloudField::PrivateServers, CloudValue::Price(Some(25)));
    let u = d.updates();
    assert_eq!(u.experience.console_enabled, Some(true));
    assert_eq!(u.experience.private_server_price_robux, Some(Some(25)));
    assert!(u.root_place.display_name.is_none() && u.place == PlaceUpdate::default());
}

#[test]
fn turning_private_servers_off_is_a_null_write() {
    let mut r = remote(1);
    r.experience.private_server_price_robux = Some(10);
    let mut d = CloudDraft::default();
    d.reset(r);
    d.set(CloudField::PrivateServers, CloudValue::Price(None));
    assert_eq!(
        d.updates().experience.private_server_price_robux,
        Some(None)
    );
}

#[test]
fn other_place_edits_stay_on_the_place() {
    let mut d = draft(2);
    d.set(CloudField::PlaceName, CloudValue::Text("New".into()));
    d.set(CloudField::ServerSize, CloudValue::Count(10));
    d.set(CloudField::Name, CloudValue::Text("Exp2".into()));
    let u = d.updates();
    assert_eq!(u.place.display_name.as_deref(), Some("New"));
    assert_eq!(u.place.server_size, Some(10));
    assert_eq!(u.root_place.display_name.as_deref(), Some("Exp2"));
}

#[test]
fn root_place_merges_places_edits_with_basic_info_winning() {
    let mut d = draft(1);
    d.set(CloudField::Name, CloudValue::Text("A".into()));
    d.set(CloudField::PlaceName, CloudValue::Text("B".into()));
    d.set(CloudField::PlaceDescription, CloudValue::Text("D".into()));
    d.set(CloudField::ServerSize, CloudValue::Count(20));
    let u = d.updates();
    assert_eq!(u.root_place.display_name.as_deref(), Some("A"));
    assert_eq!(u.root_place.description.as_deref(), Some("D"));
    assert_eq!(u.root_place.server_size, Some(20));
    assert_eq!(u.place, PlaceUpdate::default());
}

#[test]
fn scopes_are_deduplicated_in_field_order() {
    let mut d = draft(1);
    d.set(CloudField::ServerSize, CloudValue::Count(5));
    d.set(CloudField::Vr, CloudValue::Bool(true));
    d.set(CloudField::Console, CloudValue::Bool(true));
    assert_eq!(d.scopes(), vec!["universe:write", "universe.place:write"]);
}

fn dom_with(class: &str, props: &[(&str, Variant)]) -> WeakDom {
    let mut dom = WeakDom::new();
    let r = dom.new_instance(class, class, None);
    for (name, value) in props {
        dom.set_property(r, name, value.clone()).unwrap();
    }
    dom
}

#[test]
fn local_read_uses_the_property_else_the_default() {
    let db = ReflectionDatabase::embedded();
    let dom = dom_with("Players", &[("RespawnTime", Variant::Float32(9.))]);
    let d = LocalDraft::read(&dom, &db);
    assert_eq!(
        d.value(index_of("RespawnTime")),
        Some(&Variant::Float32(9.))
    );
    assert_eq!(
        d.value(index_of("CharacterAutoLoads")),
        db.default_value("Players", "CharacterAutoLoads")
    );
}

#[test]
fn local_fields_of_a_missing_service_are_none_and_ignore_edits() {
    let db = ReflectionDatabase::embedded();
    let dom = dom_with("Players", &[]);
    let mut d = LocalDraft::read(&dom, &db);
    let i = index_of("HttpEnabled");
    assert_eq!(d.value(i), None);
    d.set(i, Variant::Bool(true));
    assert!(!d.is_dirty());
}

#[test]
fn local_edits_revert_and_report_writes() {
    let db = ReflectionDatabase::embedded();
    let dom = dom_with("Players", &[("RespawnTime", Variant::Float32(9.))]);
    let mut d = LocalDraft::read(&dom, &db);
    let i = index_of("RespawnTime");
    d.set(i, Variant::Float32(3.));
    assert!(d.is_edited(i) && d.is_dirty());
    let writes = d.writes();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].0.property, "RespawnTime");
    assert_eq!(writes[0].1, Variant::Float32(3.));
    d.set(i, Variant::Float32(9.));
    assert!(!d.is_dirty() && d.writes().is_empty());
}

#[test]
fn local_rebase_follows_the_place_and_keeps_differing_edits() {
    let db = ReflectionDatabase::embedded();
    let read = |t: f32| {
        LocalDraft::read(
            &dom_with("Players", &[("RespawnTime", Variant::Float32(t))]),
            &db,
        )
    };
    let i = index_of("RespawnTime");
    let mut d = read(9.);
    assert!(!d.rebase(read(9.)));
    d.set(i, Variant::Float32(3.));
    assert!(d.rebase(read(5.)));
    assert_eq!(d.value(i), Some(&Variant::Float32(3.)));
    assert!(d.rebase(read(3.)));
    assert!(!d.is_dirty());
}

#[test]
fn parse_float_checks_the_range() {
    let db = ReflectionDatabase::embedded();
    let f = field("RespawnTime");
    assert_eq!(parse_local(f, &db, " 2.5 "), Ok(Variant::Float32(2.5)));
    assert!(parse_local(f, &db, "-1").unwrap_err().contains("3600"));
    assert!(parse_local(f, &db, "abc").is_err());
    assert!(parse_local(f, &db, "NaN").is_err());
}

#[test]
fn parse_enum_is_case_insensitive_and_lists_names_on_error() {
    let db = ReflectionDatabase::embedded();
    let f = field("GameSettingsAvatar");
    let items = db.enum_items("GameAvatarType").unwrap();
    let (name, value) = &items[0];
    assert_eq!(
        parse_local(f, &db, &name.to_uppercase()),
        Ok(Variant::Enum(*value))
    );
    let err = parse_local(f, &db, "nope").unwrap_err();
    assert!(err.contains(name.as_str()));
}

#[test]
fn parse_bool_accepts_true_and_false() {
    let db = ReflectionDatabase::embedded();
    let f = field("HttpEnabled");
    assert_eq!(parse_local(f, &db, "True"), Ok(Variant::Bool(true)));
    assert_eq!(parse_local(f, &db, "false"), Ok(Variant::Bool(false)));
    assert!(parse_local(f, &db, "yes").is_err());
}

#[test]
fn every_local_field_resolves_in_the_reflection_database() {
    let db = ReflectionDatabase::embedded();
    for f in LOCAL_FIELDS {
        // The embedded dump predates these two newer properties.
        let newer = matches!(
            f.property,
            "LoadCharacterLayeredClothing" | "EnableDynamicHeads"
        );
        let Some(p) = db.resolve_property(f.class, f.property) else {
            assert!(newer, "{}.{} unresolved", f.class, f.property);
            continue;
        };
        match f.kind {
            LocalKind::Bool => assert_eq!(p.value_type, "bool", "{}", f.property),
            LocalKind::Float { .. } => assert!(
                p.value_type == "float" || p.value_type == "double",
                "{}: {}",
                f.property,
                p.value_type
            ),
            LocalKind::Enum(name) => {
                assert_eq!(p.value_type, name, "{}", f.property);
                assert!(
                    db.enum_items(name).is_some_and(|i| !i.is_empty()),
                    "{name} has no items"
                );
            }
        }
    }
}

#[test]
fn team_create_edit_waits_for_the_first_read() {
    let mut tc = SwitchDraft::default();
    tc.set(true);
    assert_eq!((tc.value(), tc.pending()), (None, None));
    tc.reset(false);
    tc.set(true);
    assert_eq!((tc.value(), tc.pending()), (Some(true), Some(true)));
    tc.set(false);
    assert_eq!((tc.value(), tc.pending()), (Some(false), None));
}

#[test]
fn team_create_rebase_keeps_only_a_differing_edit() {
    let mut tc = SwitchDraft::default();
    tc.reset(false);
    tc.set(true);
    tc.rebase(true);
    assert_eq!((tc.value(), tc.pending()), (Some(true), None));
    tc.set(false);
    tc.rebase(true);
    assert_eq!(tc.pending(), Some(false));
    tc.reset(true);
    assert_eq!(tc.pending(), None);
}
