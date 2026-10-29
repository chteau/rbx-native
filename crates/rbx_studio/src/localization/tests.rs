use rbx_dom::{Ref, Variant, WeakDom};

use super::strings::assigned_text;
use super::*;

fn string(dom: &mut WeakDom, at: Ref, name: &str, value: &str) {
    dom.set_property(at, name, Variant::String(value.into()))
        .unwrap();
}

/// `StarterGui.Shop` holding a title, a price with no letters in it, a
/// search box, and a frame with `AutoLocalize` off around a player name.
fn place() -> (WeakDom, Ref, Ref) {
    let mut dom = WeakDom::new();
    let starter = dom.new_instance("StarterGui", "StarterGui", None);
    let shop = dom.new_instance("ScreenGui", "Shop", Some(starter));
    let title = dom.new_instance("TextLabel", "Title", Some(shop));
    string(&mut dom, title, "Text", "Buy gems");
    let price = dom.new_instance("TextLabel", "Price", Some(shop));
    string(&mut dom, price, "Text", "100");
    let search = dom.new_instance("TextBox", "Search", Some(shop));
    string(&mut dom, search, "Text", "typed by a player");
    string(&mut dom, search, "PlaceholderText", "Search items");
    let names = dom.new_instance("Frame", "Names", Some(shop));
    dom.set_property(names, "AutoLocalize", Variant::Bool(false))
        .unwrap();
    let player = dom.new_instance("TextLabel", "Player", Some(names));
    string(&mut dom, player, "Text", "Builderman");
    let script = dom.new_instance("LocalScript", "ShopClient", Some(shop));
    string(
        &mut dom,
        script,
        "Source",
        "local title = script.Parent.Title\n\
         -- title.Text = \"commented out\"\n\
         title.Text = \"Sold out\"\n\
         print(title.Text)\n\
         script.Parent.Price.Text = 'Free \\'today\\''\n\
         title.Text = tostring(5)\n",
    );
    let server = dom.new_instance("Script", "Server", Some(shop));
    string(
        &mut dom,
        server,
        "Source",
        "workspace.Sign.Text = \"server only\"",
    );
    (dom, title, script)
}

#[test]
fn collects_auto_localized_gui_text_and_client_text_setters() {
    let (dom, title, script) = place();
    let found = collect(&dom);
    let texts: Vec<&str> = found.iter().map(|f| f.text.as_str()).collect();
    assert_eq!(
        texts,
        ["Buy gems", "Search items", "Sold out", "Free 'today'"]
    );
    assert_eq!(found[0].context, "StarterGui.Shop.Title");
    assert_eq!(found[0].origin, Origin::Gui(title));
    assert_eq!(found[2].origin, Origin::Script(script, 3));
    assert_eq!(found[3].origin, Origin::Script(script, 5));
}

#[test]
fn only_literal_text_assignments_count() {
    let source = "a.Text, b.Name = \"one\", \"two\"\nlocal Text = \"x\"\nc.Text ..= \"y\"\nd.Text = `hi {n}`";
    assert_eq!(assigned_text(source), [("one".to_string(), 1)]);
}

#[test]
fn table_json_round_trips_and_omits_empty_fields() {
    let json = r#"[{"key":"Greeting","source":"Hello","values":{"es-es":"Hola"},"comment":"kept"},{"context":"Gui.Title","examples":"","source":"Play","values":{}}]"#;
    let entries = parse(json).unwrap();
    assert_eq!(entries[0].key, "Greeting");
    assert_eq!(entries[0].values["es-es"], "Hola");
    assert_eq!(entries[1].context, "Gui.Title");
    let written = serialize(&entries);
    assert_eq!(
        written,
        r#"[{"key":"Greeting","source":"Hello","values":{"es-es":"Hola"},"comment":"kept"},{"context":"Gui.Title","source":"Play","values":{}}]"#
    );
    assert_eq!(parse(&written).unwrap(), entries);
    assert!(parse("").unwrap().is_empty());
    assert!(parse("not json").is_err());
}

#[test]
fn writing_makes_the_service_and_table_then_merges_only_new_strings() {
    let (mut dom, title, _) = place();
    let found = collect(&dom);
    let (table, added) = write(&mut dom, &found).unwrap();
    assert_eq!(added, 4);
    let instance = dom.get(table).unwrap();
    assert_eq!(instance.class(), "LocalizationTable");
    let service = dom.get(dom.parent(table).unwrap()).unwrap();
    assert_eq!(service.class(), "LocalizationService");
    assert_eq!(
        instance.properties().get("SourceLocaleId"),
        Some(&Variant::String("en-us".into()))
    );
    let Some(Variant::String(json)) = instance.properties().get("Contents") else {
        panic!("no Contents written");
    };
    let entries = parse(json).unwrap();
    assert_eq!(entries[0].source, "Buy gems");
    assert_eq!(entries[0].context, "StarterGui.Shop.Title");
    assert!(
        json.starts_with(r#"[{"context":"StarterGui.Shop.Title","source":"Buy gems","values":{}}"#)
    );

    // Again: everything is in the table now, and nothing is written twice.
    let (again, added) = write(&mut dom, &found).unwrap();
    assert_eq!((again, added), (table, 0));

    // A new label merges into the same table, after what was there.
    let gui = dom.parent(title).unwrap();
    let label = dom.new_instance("TextButton", "Close", Some(gui));
    string(&mut dom, label, "Text", "Close");
    let found = collect(&dom);
    let (_, added) = write(&mut dom, &found).unwrap();
    assert_eq!(added, 1);
    let Some(Variant::String(json)) = dom.get(table).unwrap().properties().get("Contents") else {
        panic!();
    };
    let sources: Vec<String> = parse(json).unwrap().into_iter().map(|e| e.source).collect();
    assert_eq!(sources.last().map(String::as_str), Some("Close"));
    assert_eq!(sources.len(), 5);
}

#[test]
fn strings_already_in_any_table_are_skipped() {
    let (mut dom, _, _) = place();
    let storage = dom.new_instance("ReplicatedStorage", "ReplicatedStorage", None);
    let other = dom.new_instance("LocalizationTable", "Shop", Some(storage));
    string(
        &mut dom,
        other,
        "Contents",
        r#"[{"source":"Buy gems","values":{"fr-fr":"Acheter"}}]"#,
    );
    let found = collect(&dom);
    let new = new_entries(&found, &known_sources(&dom));
    let sources: Vec<&str> = new.iter().map(|e| e.source.as_str()).collect();
    assert_eq!(sources, ["Search items", "Sold out", "Free 'today'"]);
    assert_eq!(
        new[0],
        Entry {
            source: "Search items".into(),
            context: "StarterGui.Shop.Search".into(),
            ..Entry::default()
        }
    );
}

#[test]
fn a_table_that_does_not_parse_is_left_alone() {
    let (mut dom, _, _) = place();
    let service = dom.new_instance("LocalizationService", "LocalizationService", None);
    let table = dom.new_instance("LocalizationTable", "Broken", Some(service));
    string(&mut dom, table, "Contents", "{oops");
    let found = collect(&dom);
    assert!(write(&mut dom, &found).is_err());
    assert_eq!(
        dom.get(table).unwrap().properties().get("Contents"),
        Some(&Variant::String("{oops".into()))
    );
}
