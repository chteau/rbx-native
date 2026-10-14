use rbx_dom::WeakDom;
use rbx_reflection::ReflectionDatabase;

use crate::{Output, PrintPart, PrintValue, Runtime, TableDump};

fn run(source: &str) -> Output {
    let mut runtime =
        Runtime::new(WeakDom::new(), ReflectionDatabase::embedded()).expect("runtime");
    runtime.run(source).expect("script runs")
}

fn only_table(output: &Output) -> &TableDump {
    match output.printed()[0].as_slice() {
        [PrintPart::Table(table)] => table,
        other => panic!("expected one table, got {other:?}"),
    }
}

fn shown(value: &PrintValue) -> &str {
    match value {
        PrintValue::Text(text) => text,
        other => panic!("expected text, got {other:?}"),
    }
}

#[test]
fn a_printed_table_keeps_its_entries_and_its_line_keeps_tostring() {
    let output = run(r#"print("t", {b = "x", a = 1, [2] = true, [1] = 3})"#);
    let table = match &output.printed()[0][1] {
        PrintPart::Table(table) => table,
        other => panic!("expected a table, got {other:?}"),
    };
    let keys: Vec<&str> = table.entries.iter().map(|e| e.key.as_str()).collect();
    assert_eq!(keys, ["1", "2", "\"a\"", "\"b\""]);
    assert_eq!(shown(&table.entries[3].value), "\"x\"");
    assert_eq!(shown(&table.entries[0].value), "3");
    assert_eq!(output.lines()[0], format!("t table: {}", table.address));
    assert!(table.address.starts_with("0x"));
}

#[test]
fn the_address_matches_tostring() {
    let output = run("local t = {} print(t) print(tostring(t))");
    let table = only_table(&output);
    assert_eq!(output.lines()[1], format!("table: {}", table.address));
}

#[test]
fn nested_tables_nest_and_cycles_stop_at_a_reference() {
    let output = run("local t = {inner = {1}} t.self = t print(t)");
    let table = only_table(&output);
    match &table.entries[0].value {
        PrintValue::Table(inner) => assert_eq!(shown(&inner.entries[0].value), "1"),
        other => panic!("expected a nested table, got {other:?}"),
    }
    assert_eq!(
        table.entries[1].value,
        PrintValue::Reference(table.address.clone())
    );
}

#[test]
fn a_table_with_its_own_tostring_prints_as_text() {
    let output = run(r#"print(setmetatable({}, {__tostring = function() return "Thing" end}))"#);
    assert_eq!(output.printed()[0], vec![PrintPart::Text("Thing".into())]);
    assert_eq!(output.lines()[0], "Thing");
}

#[test]
fn a_huge_table_counts_what_it_does_not_walk() {
    let output = run("local t = {} for i = 1, 250 do t[i] = i end print(t)");
    let table = only_table(&output);
    assert_eq!(table.entries.len(), 200);
    assert_eq!(table.more, 50);
}
