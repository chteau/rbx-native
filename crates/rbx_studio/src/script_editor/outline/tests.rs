use super::*;

/// The byte offset of the `nth` (0-based) whole-word occurrence of `needle`.
fn at(source: &str, needle: &str, nth: usize) -> usize {
    let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    source
        .match_indices(needle)
        .map(|(start, _)| start)
        .filter(|&start| {
            !word(source[..start].chars().next_back())
                && !word(source[start + needle.len()..].chars().next())
        })
        .nth(nth)
        .unwrap()
}

fn jump(source: &str, needle: &str, nth: usize) -> Option<usize> {
    declaration(source, at(source, needle, nth)).map(|range| range.start)
}

#[test]
fn lists_every_named_function_with_its_full_path() {
    let source = "local function a() end\n\
                  function M.b(x) end\n\
                  function M:c() end\n\
                  local d = function() end\n\
                  M.e = function() end\n\
                  task.spawn(function() end)\n";
    let names: Vec<String> = functions(source).into_iter().map(|f| f.name).collect();
    assert_eq!(names, ["a", "M.b", "M:c", "d", "M.e"]);
    let c = &functions(source)[2];
    assert_eq!(&source[c.range.clone()], "c");
}

#[test]
fn a_call_jumps_to_its_local_declaration() {
    let source = "local count = 0\nlocal function bump() count += 1 end\nbump()\n";
    assert_eq!(jump(source, "bump", 1), Some(at(source, "bump", 0)));
    assert_eq!(jump(source, "count", 1), Some(at(source, "count", 0)));
}

#[test]
fn an_inner_local_shadows_an_outer_one_only_inside_its_block() {
    let source = "local x = 1\nif true then\n\tlocal x = 2\n\tprint(x)\nend\nprint(x)\n";
    assert_eq!(jump(source, "x", 2), Some(at(source, "x", 1)));
    assert_eq!(jump(source, "x", 3), Some(at(source, "x", 0)));
}

#[test]
fn parameters_and_loop_variables_are_scoped_to_their_body() {
    let source = "local function f(a: number, b)\n\treturn a + b\nend\n\
                  for i, v in pairs(t) do print(i, v) end\nprint(a)\n";
    assert_eq!(jump(source, "a", 1), Some(at(source, "a", 0)));
    assert_eq!(jump(source, "b", 1), Some(at(source, "b", 0)));
    assert_eq!(jump(source, "v", 1), Some(at(source, "v", 0)));
    // `a` outside `f` is a global nothing declares.
    assert_eq!(jump(source, "a", 2), None);
}

#[test]
fn a_member_call_jumps_to_the_function_statement() {
    let source = "local helper = 1\nlocal M = {}\nfunction M.helper() end\nM.helper()\n";
    assert_eq!(jump(source, "helper", 2), Some(at(source, "helper", 1)));
}

#[test]
fn a_global_function_declared_later_is_found() {
    let source = "run()\nfunction run() end\n";
    assert_eq!(jump(source, "run", 0), Some(at(source, "run", 1)));
}

#[test]
fn a_typed_local_without_initializer_stops_at_the_line_end() {
    let source = "local x: number\ny, z = 1, 2\nprint(z)\n";
    assert_eq!(jump(source, "z", 1), None);
}

#[test]
fn nothing_under_the_offset_is_none() {
    assert_eq!(declaration("print(1)", 6), None);
    assert_eq!(declaration("-- just words", 5), None);
}
