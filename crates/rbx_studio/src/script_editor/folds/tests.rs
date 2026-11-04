use super::*;

/// The script Roblox Studio was measured on: `FoldFind.rbxlx`,
/// `ServerScriptService.Main`.
const STUDIO: &str = r#"-- Main: round loop
--[[
  Multi-line comment
  that folds
]]
local Players = game:GetService("Players")
local Util = require(game.ReplicatedStorage.Util)

local config = {
	health = 100,
	speed = 16,
	colors = {
		"Red",
		"Blue",
	},
}

local function onPlayerAdded(player)
	if player.Name == "Builderman" then
		print("hi", config.health)
	elseif player.Name == "Guest" then
		print("guest")
	else
		print("other")
	end
	for i = 1, 3 do
		print(i)
	end
end

Players.PlayerAdded:Connect(onPlayerAdded)

while false do
	task.wait(1)
end

repeat
	local health = Util.getHealth()
until health > 0

do
	local text = [[
long string
lines
]]
	print(text)
end

local door = workspace.Part
local other = workspace:FindFirstChild("Part")
local waited = workspace:WaitForChild("Part")
print(door, other, waited)
"#;

/// 1-based `(opener, closer)` pairs, the way the editor's line numbers read.
fn pairs(text: &str) -> Vec<(usize, usize)> {
    fold_ranges(text)
        .iter()
        .map(|r| (r.start_line + 1, r.end_line + 1))
        .collect()
}

#[test]
fn studio_script_folds_where_studio_does() {
    assert_eq!(
        pairs(STUDIO),
        vec![
            (2, 5),   // --[[ ]] comment, hides 3-4
            (9, 16),  // table
            (12, 15), // nested table
            (18, 29), // local function
            (19, 21), // if, up to the elseif
            (21, 23), // elseif, up to the else
            (23, 25), // else, up to the end
            (26, 28), // numeric for
            (33, 35), // while
            (37, 39), // repeat, closer is the until line
            (41, 47), // do
            (42, 45), // long string
        ]
    );
}

#[test]
fn hidden_lines_leave_opener_and_closer_visible() {
    // Studio collapses the table on 9-16 to `local config = {` then `}`.
    let r = fold_ranges(STUDIO)[1];
    assert_eq!((r.start_line, r.end_line), (8, 15));
}

#[test]
fn adjacent_lines_have_nothing_to_hide() {
    assert_eq!(pairs("local t = {\n}\nif x then\nend\n"), vec![]);
    assert_eq!(pairs("do\n\tprint(1)\nend"), vec![(1, 3)]);
}

#[test]
fn function_forms() {
    let src = "function a()\n\tx()\nend\nlocal function b()\n\tx()\nend\nlocal c = function()\n\tx()\nend\nfunction t:m()\n\tx()\nend\n";
    assert_eq!(pairs(src), vec![(1, 3), (4, 6), (7, 9), (10, 12)]);
}

#[test]
fn generic_for_and_long_bracket_forms() {
    let src = "for k, v in pairs(t) do\n\tx()\nend\nlocal s = [==[\na\nb\n]==]\n--[=[\nc\nd\n]=]\n";
    assert_eq!(pairs(src), vec![(1, 3), (4, 7), (8, 11)]);
}

#[test]
fn single_line_comments_and_quoted_strings_do_not_fold() {
    assert_eq!(pairs("-- a\n-- b\n-- c\nlocal s = \"x\"\n"), vec![]);
}

#[test]
fn outermost_wins_when_openers_share_a_line() {
    let src = "local t = { f = function()\n\tx()\nend }\n";
    // The table closes on line 3 as well; either way one fold, line 1.
    assert_eq!(pairs(src), vec![(1, 3)]);
    let src = "local t = { f = function()\n\tx()\n\ty()\nend,\n\t}\n";
    assert_eq!(pairs(src), vec![(1, 5)]);
}

#[test]
fn an_if_without_branches_folds_to_its_end() {
    assert_eq!(pairs("if x then\n\ty()\nend\n"), vec![(1, 3)]);
}

#[test]
fn half_typed_script_still_folds_what_parsed() {
    // The first function is complete; the second never closes.
    let src = "local function a()\n\tx()\nend\n\nlocal t = {\n\t1,\n\t2,\n}\n\nlocal function b()\n\tif x then\n";
    let got = pairs(src);
    assert!(got.contains(&(1, 3)), "{got:?}");
    assert!(got.contains(&(5, 8)), "{got:?}");
}

#[test]
fn empty_and_unparseable_text_gives_nothing() {
    assert!(fold_ranges("").is_empty());
    assert!(pairs("@@@").is_empty());
}

#[test]
fn ranges_are_sorted_with_unique_start_lines() {
    let got = fold_ranges(STUDIO);
    assert!(got.windows(2).all(|w| w[0].start_line < w[1].start_line));
}

#[test]
fn crlf_line_endings_count_one_line_each() {
    let src = "local t = {\r\n\t1,\r\n}\r\n";
    assert_eq!(pairs(src), vec![(1, 3)]);
}

#[test]
fn text_past_the_cap_is_not_parsed() {
    let big = format!("do\n{}\nend\n", "x()\n".repeat(MAX_HIGHLIGHTED_BYTES / 4));
    assert!(fold_ranges(&big).is_empty());
}
