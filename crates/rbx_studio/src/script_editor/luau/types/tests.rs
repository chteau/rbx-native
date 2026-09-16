use crate::script_editor::luau::{tokenize, TokenKind};

/// Every name the type pass claimed, in source order, with whether Luau
/// declares it itself.
fn types(source: &str) -> Vec<(&str, TokenKind)> {
    tokenize(source)
        .into_iter()
        .filter(|token| matches!(token.kind, TokenKind::Type | TokenKind::BuiltinType))
        .map(|token| (&source[token.range], token.kind))
        .collect()
}

/// Just the names, for the many cases where which kind of type it is is not
/// what is under test.
fn named(source: &str) -> Vec<&str> {
    types(source).into_iter().map(|(text, _)| text).collect()
}

#[test]
fn a_local_annotation_is_a_type_position() {
    assert_eq!(named("local n: Vector3 = v"), ["Vector3"]);
    assert_eq!(
        named("local a: string, b: number = '', 0"),
        ["string", "number"],
        "one `local` can annotate every name in its list"
    );
}

#[test]
fn a_parameters_annotation_is_a_type_position() {
    assert_eq!(
        named("function f(x: Part, y: CFrame) end"),
        ["Part", "CFrame"]
    );
    assert_eq!(
        named("local f = function(x: Part) end"),
        ["Part"],
        "an anonymous function annotates its parameters the same way"
    );
}

#[test]
fn a_return_annotation_is_a_type_position() {
    assert_eq!(named("function f(): Instance end"), ["Instance"]);
    assert_eq!(
        named("function f(x: number): number end"),
        ["number", "number"]
    );
}

#[test]
fn a_type_declaration_names_a_type_either_side_of_its_equals() {
    assert_eq!(named("type Name = string"), ["Name", "string"]);
    assert_eq!(
        named("export type Handler<T> = (T) -> ()"),
        ["Handler", "T", "T"]
    );
}

#[test]
fn a_cast_is_a_type_position() {
    assert_eq!(named("local x = y :: Part"), ["Part"]);
    assert_eq!(named("local n = (v :: any) + 1"), ["any"]);
}

#[test]
fn builtin_types_are_told_apart_from_declared_ones() {
    assert_eq!(
        types("local x: number = y :: Vector3"),
        [
            ("number", TokenKind::BuiltinType),
            ("Vector3", TokenKind::Type)
        ]
    );
}

#[test]
fn a_method_calls_colon_is_not_an_annotation() {
    // The one genuinely ambiguous token in Luau: `obj:method()` and
    // `local x: T` spell it identically, and only position separates them.
    assert!(named("obj:method(1)").is_empty());
    assert!(named("local v = workspace:FindFirstChild('Part')").is_empty());
    assert!(named("game:GetService('Players')").is_empty());
}

#[test]
fn an_expression_is_never_a_type_position() {
    assert!(
        named("local t = { foo = 1, bar = Vector3.new() }").is_empty(),
        "a table constructor is a value, however type-like the names in it look"
    );
    assert!(
        named("if a < b then print(c) end").is_empty(),
        "a comparison is not a generic parameter list"
    );
    assert!(named("local ok = a and b or c").is_empty());
}

#[test]
fn an_annotation_stops_at_the_value_it_annotates() {
    assert_eq!(
        named("local x: number = Vector3.new(1, 2, 3)"),
        ["number"],
        "the initialiser is an expression again, not more of the type"
    );
}

#[test]
fn a_union_or_intersection_names_every_member() {
    assert_eq!(named("type X = A | B | C"), ["X", "A", "B", "C"]);
    assert_eq!(named("local x: Part & Named = y"), ["Part", "Named"]);
}

#[test]
fn a_table_type_names_its_field_types_but_not_its_field_names() {
    assert_eq!(
        named("local t: { [string]: number } = {}"),
        ["string", "number"]
    );
    assert_eq!(
        named("type Row = { name: string, at: Vector3 }"),
        ["Row", "string", "Vector3"],
        "`name` and `at` are fields, not types"
    );
    assert_eq!(
        named("local grown: { Grown } = {}"),
        ["Grown"],
        "the array shorthand has no field name to skip"
    );
}

#[test]
fn a_function_type_names_its_parameters_and_its_result() {
    assert_eq!(
        named("local cb: (number, string) -> boolean"),
        ["number", "string", "boolean"],
        "a function type may name none of its parameters"
    );
    assert_eq!(
        named("local cb: (self: Part, n: number) -> ()"),
        ["Part", "number"],
        "or all of them"
    );
}

#[test]
fn optionals_generics_and_qualified_names_stay_inside_one_annotation() {
    assert_eq!(
        named("local x: Map<string, Part>? = nil"),
        ["Map", "string", "Part"]
    );
    assert_eq!(
        named("local e: Roact.Element = nil"),
        ["Roact", "Element"],
        "a qualified name is one type in two halves"
    );
}

#[test]
fn a_generic_functions_parameters_are_types_wherever_they_appear() {
    assert_eq!(
        named("function f<T, U>(a: T, b: U): T end"),
        ["T", "U", "T", "U", "T"]
    );
}

#[test]
fn typeofs_argument_is_an_expression_not_a_type() {
    assert!(
        named("type Config = typeof(defaults)").contains(&"Config"),
        "the declared name is still a type"
    );
    assert!(
        !named("type Config = typeof(defaults)").contains(&"defaults"),
        "`typeof` takes a value, so what is inside it keeps the colour it had"
    );
}

#[test]
fn the_contextual_type_keyword_is_only_a_declaration_where_one_follows() {
    // `type` is reserved only in context — Luau still lets it be a call and a
    // variable name, and neither is followed by the name a declaration needs.
    assert!(named("local kind = type(x)").is_empty());
    assert!(named("local type = 1").is_empty());
}

#[test]
fn an_annotation_inside_an_interpolation_hole_is_still_found() {
    // A hole re-enters the lexer, so it gets its own type pass rather than
    // inheriting the outer one's position.
    assert_eq!(named("`{v :: Vector3}`"), ["Vector3"]);
}
