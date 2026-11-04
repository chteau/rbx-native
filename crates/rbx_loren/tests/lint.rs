mod common;

use std::cell::RefCell;
use std::fs;
use std::io::{Cursor, Write};
use std::path::Path;
use std::rc::Rc;

use common::{list_files, now, project, read, STAMP};
use rbx_loren::ctx::DoctorOptions;
use rbx_loren::lint::{
    apply_edits, run_doctor, scan_text, structure, tokenize, Issue, Severity, TokenKind,
};
use rbx_loren::log::Log;
use rbx_loren::prompt::Prompter;
use regex::Regex;

// Not ported: the "minimal logger" test (a JS duck-typed logger object; `Log` is concrete here).

fn lines(parts: &[&str]) -> String {
    parts.join("\n")
}

fn matches(text: &str, pattern: &str) -> bool {
    Regex::new(pattern).expect("pattern").is_match(text)
}

fn assert_match(text: &str, pattern: &str) {
    assert!(
        matches(text, pattern),
        "{text:?} does not match /{pattern}/"
    );
}

fn scan(src: &str) -> Vec<Issue> {
    scan_text(src, "module.luau", false, false)
}

fn colon(src: &str) -> Vec<Issue> {
    scan(src)
        .into_iter()
        .filter(|i| i.kind == "colon-middleware")
        .collect()
}

fn fix_all(src: &str) -> String {
    let edits: Vec<_> = colon(src)
        .into_iter()
        .filter(|i| !i.manual)
        .filter_map(|i| i.edit)
        .collect();
    apply_edits(src, &edits)
}

fn kinds(issues: &[Issue]) -> Vec<String> {
    issues
        .iter()
        .map(|i| format!("{}:{}", i.kind, i.line))
        .collect()
}

fn name_of(i: &Issue) -> String {
    let re = Regex::new(r"Middleware[.:](\w+)").unwrap();
    re.captures(&i.message)
        .map(|c| c[1].to_string())
        .unwrap_or_default()
}

fn name_lines(found: &[Issue]) -> Vec<String> {
    found
        .iter()
        .map(|i| format!("{}:{}", name_of(i), i.line))
        .collect()
}

fn preview_of(i: &Issue) -> String {
    let after = i.message.split("Dot style: ").nth(1).unwrap_or_default();
    after
        .split(". Its body uses self")
        .next()
        .unwrap_or_default()
        .to_string()
}

fn snapshot(dir: &Path) -> Vec<(String, String)> {
    list_files(dir)
        .into_iter()
        .map(|f| (f.clone(), read(&dir.join(&f))))
        .collect()
}

fn write(dir: &Path, rel: &str, text: &str) {
    let p = dir.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

fn opts(log: &Log) -> DoctorOptions<'_> {
    DoctorOptions {
        fix: false,
        yes: false,
        is_tty: false,
        prompt: None,
        log,
        server_dir: None,
        src_dir: None,
        legacy: false,
        now: None,
    }
}

#[derive(Clone, Default)]
struct Shared(Rc<RefCell<Vec<u8>>>);

impl Write for Shared {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn prompter(is_tty: bool, answer: &str, asked: &Shared) -> Prompter {
    Prompter::new(
        false,
        is_tty,
        Box::new(Cursor::new(answer.as_bytes().to_vec())),
        Box::new(asked.clone()),
    )
}

fn asked_text(asked: &Shared) -> String {
    String::from_utf8_lossy(&asked.0.borrow()).into_owned()
}

// Tokenizer and structure -------------------------------------------------------------------------

#[test]
fn tokenizer_drops_comments_and_keeps_strings_as_single_tokens() {
    let src = lines(&[
        "-- function S.Middleware:A(p) end",
        "--[==[ function S.Middleware:B(p) end ]==]",
        "local a = \"function S.Middleware:C(p) end\"",
        "local b = 'x\\'y'",
        "local c = [[ function S.Middleware:D(p) end ]]",
        "local d = `{S.Middleware} {\"}\"} text`",
        "local e = 1e-5 + 0x1F + .5",
    ]);
    let toks = tokenize(&src);
    let names: Vec<&str> = toks
        .iter()
        .filter(|t| t.kind == TokenKind::Name)
        .map(|t| t.value.as_str())
        .collect();
    assert_eq!(names, ["a", "b", "c", "d", "e"]);
    assert_eq!(toks.iter().filter(|t| t.kind == TokenKind::Str).count(), 4);
    let nums: Vec<&str> = toks
        .iter()
        .filter(|t| t.kind == TokenKind::Num)
        .map(|t| t.value.as_str())
        .collect();
    assert_eq!(nums, ["1e-5", "0x1F", ".5"]);
    assert_eq!(toks.iter().find(|t| t.value == "e").unwrap().line, 7);
    assert_eq!(colon(&src).len(), 0);
}

#[test]
fn structure_matches_blocks_including_if_expressions_that_have_no_end() {
    let src = lines(&[
        "local x = if a then 1 else 2",
        "local function f()",
        "\tlocal y = if b then function() if c then return end end else nil",
        "\trepeat local z = 1 until if z then true else false",
        "\twhile true do break end",
        "end",
        "return f",
    ]);
    let toks = tokenize(&src);
    let s = structure(&toks);
    let f = toks.iter().position(|t| t.value == "function").unwrap();
    assert_eq!(
        toks[s.matching[f] as usize].line, 6,
        "local function f() closes on line 6"
    );
    let ret = toks
        .iter()
        .position(|t| t.value == "return" && t.line == 7)
        .unwrap();
    assert_eq!(s.depth[ret], 0, "the file ends at the top level");
}

// Colon middleware --------------------------------------------------------------------------------

#[test]
fn function_x_middleware_colon_name_is_found_at_any_arity_and_rewritten_to_a_dot() {
    let src = lines(&[
        "local S = { Client = {}, Middleware = {} }",
        "function S.Client:A() end",
        "function S.Client:B(p) end",
        "function S.Client:C(p, a, b) end",
        "function S.Middleware:A() return true end",
        "function S.Middleware:B(p) return true end",
        "function S.Middleware:C(p, a, b) return true end",
        "return S",
    ]);
    let found: Vec<Issue> = scan_text(&src, "src/server/Services/S.luau", false, false)
        .into_iter()
        .filter(|i| i.kind == "colon-middleware")
        .collect();
    let got: Vec<String> = found
        .iter()
        .map(|i| {
            format!(
                "{}:{}:{}",
                name_of(i),
                i.line,
                if i.severity == Severity::Fixable {
                    "fixable"
                } else {
                    "warn"
                }
            )
        })
        .collect();
    assert_eq!(got, ["A:5:fixable", "B:6:fixable", "C:7:fixable"]);
    assert_eq!(preview_of(&found[2]), "function S.Middleware.C(p, a, b)");
    assert_match(
        &found[2].message,
        r"S\.Middleware:C is colon-style.*denies every C call.*Dot style: function S\.Middleware\.C\(p, a, b\)",
    );
    let fixed = fix_all(&src);
    let got: Vec<&str> = fixed.split('\n').skip(4).take(3).collect();
    assert_eq!(
        got.join("\n"),
        lines(&[
            "function S.Middleware.A() return true end",
            "function S.Middleware.B(p) return true end",
            "function S.Middleware.C(p, a, b) return true end",
        ])
    );
    assert_eq!(colon(&fixed).len(), 0);
}

#[test]
fn name_function_self_inside_a_middleware_table_drops_self_nested_functions_are_untouched() {
    let src = lines(&[
        "local S = {",
        "\tMiddleware = {",
        "\t\tA = function(self)",
        "\t\t\treturn true",
        "\t\tend,",
        "\t\tB = function(self: any, player: Player, n: { [string]: number })",
        "\t\t\tlocal obj = { check = function(self) return self end }",
        "\t\t\tlocal g = function(self, x) return x end",
        "\t\t\tsetmetatable({}, { __index = function(self, k) return nil end })",
        "\t\t\treturn obj:check() ~= nil",
        "\t\tend;",
        "\t\t[\"C\"] = function(",
        "\t\t\tself,",
        "\t\t\tplayer",
        "\t\t)",
        "\t\t\treturn player",
        "\t\tend,",
        "\t\tD = function(player, self) return true end,",
        "\t\tE = helper(function(self) end),",
        "\t},",
        "}",
        "return S",
    ]);
    let found = colon(&src);
    assert_eq!(name_lines(&found), ["A:3", "B:6", "C:12"]);
    assert_eq!(
        preview_of(&found[1]),
        "B = function(player: Player, n: { [string]: number })"
    );
    let fixed_src = fix_all(&src);
    let fixed: Vec<&str> = fixed_src.split('\n').collect();
    assert_eq!(fixed[2], "\t\tA = function()");
    assert_eq!(
        fixed[5],
        "\t\tB = function(player: Player, n: { [string]: number })"
    );
    assert_eq!(
        fixed[6], "\t\t\tlocal obj = { check = function(self) return self end }",
        "nested method untouched"
    );
    assert_eq!(
        fixed[7], "\t\t\tlocal g = function(self, x) return x end",
        "nested local function untouched"
    );
    assert_eq!(
        fixed[8],
        "\t\t\tsetmetatable({}, { __index = function(self, k) return nil end })"
    );
    assert_eq!(
        fixed[11..15],
        [
            "\t\t[\"C\"] = function(",
            "\t\t\tplayer",
            "\t\t)",
            "\t\t\treturn player"
        ]
    );
    assert_eq!(
        fixed[16], "\t\tD = function(player, self) return true end,",
        "self as a later parameter is not middleware style"
    );
    assert_eq!(fixed[17], "\t\tE = helper(function(self) end),");
}

#[test]
fn explicit_self_dot_forms_and_multi_line_signatures() {
    let src = lines(&[
        "local S = {}",
        "function S.Middleware.A(self, p)",
        "\treturn p",
        "end",
        "S.Middleware.B = function(self, p, q)",
        "\treturn q",
        "end",
        "function S.Middleware",
        "\t:C(",
        "\t\tp,",
        "\t\tq",
        "\t)",
        "\treturn q",
        "end",
        "function S.Middleware.D(p, q) return q end",
        "return S",
    ]);
    let found = colon(&src);
    assert_eq!(name_lines(&found), ["A:2", "B:5", "C:8"]);
    assert_eq!(preview_of(&found[2]), "function S.Middleware.C(p, q)");
    let fixed_src = fix_all(&src);
    let fixed: Vec<&str> = fixed_src.split('\n').collect();
    assert_eq!(fixed[1], "function S.Middleware.A(p)");
    assert_eq!(fixed[4], "S.Middleware.B = function(p, q)");
    assert_eq!(fixed[7..9], ["function S.Middleware", "\t.C("]);
    assert_eq!(colon(&fixed_src).len(), 0);
}

#[test]
fn a_body_that_uses_self_is_reported_for_a_hand_rewrite_never_auto_fixed() {
    let src = lines(&[
        "local S = {}",
        "function S.Middleware:A(p)",
        "\treturn self:Check(p)",
        "end",
        "function S.Middleware:B(p)",
        "\tlocal t = { self = 1 }",
        "\tlocal u = t.self",
        "\tlocal o = {}",
        "\tfunction o:m() return self end",
        "\treturn p",
        "end",
        "function S.Middleware:C(p)",
        "\tlocal self = S",
        "\treturn self:Check(p)",
        "end",
        "return S",
    ]);
    let found = colon(&src);
    let got: Vec<String> = found
        .iter()
        .map(|i| format!("{}:{}", name_of(i), i.manual))
        .collect();
    assert_eq!(got, ["A:true", "B:false", "C:false"]);
    assert_eq!(found[0].edit, None);
    assert_match(
        &found[0].message,
        r"Its body uses self, so rewrite it by hand \(use module for the Service\)",
    );
    assert_match(&fix_all(&src), r"function S\.Middleware:A\(p\)");
}

#[test]
fn self_in_an_interpolated_string_a_type_or_after_a_nested_block_local_is_a_use_hand_rewrite() {
    let func = |sig: &str, body: &str| {
        format!(
            "local S = {{}}\nfunction S.Middleware{sig}\n{body}\n\treturn true\nend\nreturn S\n"
        )
    };
    let manual = |src: &str| colon(src).iter().map(|i| i.manual).collect::<Vec<_>>();
    assert_eq!(
        manual(&func(":A(p)", "\tprint(`{self.Name} buys`)")),
        [true],
        "interpolation"
    );
    assert_eq!(
        manual(&func(":A(p)", "\tprint(`a {`b {self}`} c`)")),
        [true],
        "nested interpolation"
    );
    assert_eq!(
        manual(&func(":A(p)", "\tprint(`{p.Name} {x.self} \\{self}`)")),
        [false],
        "x.self and an escaped brace are not uses"
    );
    assert_eq!(
        manual(&func(":A(p: typeof(self))", "")),
        [true],
        "parameter type"
    );
    assert_eq!(
        manual(&func(":A(p): typeof(self)", "")),
        [true],
        "return type"
    );
    assert_eq!(
        manual(&func(
            ":A(p)",
            "\tif p then local self = 1 end\n\tprint(self)"
        )),
        [true],
        "a nested local only shadows its block"
    );
    assert_eq!(
        manual(&func(":A(p)", "\tlocal self = S\n\tprint(self)")),
        [false],
        "a body-level local shadows the rest"
    );
    assert_eq!(
        manual(&func(
            ":A(p)",
            "\tlocal self: typeof(S) = self or S\n\tprint(self)"
        )),
        [true],
        "but its value may read self"
    );
    assert_eq!(
        manual(&func(".A(self: typeof(S), p)", "\tprint(p)")),
        [false],
        "the dropped self parameter's own type is not a use"
    );
    let src = func(":A(p)", "\tprint(`{self.Name}`)");
    assert_eq!(fix_all(&src), src, "never rewritten");
    assert_eq!(
        manual("local S = {}\nfunction S.Middleware:A(p)\n\treturn (p\nreturn S\n"),
        [true]
    );
}

#[test]
fn a_bare_local_middleware_is_only_linted_when_the_file_assigns_it_to_a_module() {
    let own = lines(&[
        "local Middleware = {}",
        "Middleware.__index = Middleware",
        "function Middleware:Greet(name) return name end",
        "Middleware.Run = function(self, x) return x end",
        "local Other = { Middleware = 1 }",
        "return Middleware",
    ]);
    assert!(
        colon(&own).is_empty(),
        "the user's own class keeps its methods"
    );
    assert!(colon(
        "local Middleware = { Greet = function(self, name) return name end }\nreturn Middleware\n"
    )
    .is_empty());

    let handed = "local Middleware = {}\nfunction Middleware:Buy(p) return true end\nlocal S = { Middleware = Middleware }\nreturn S\n";
    assert_eq!(name_lines(&colon(handed)), ["Buy:2"]);
    let assigned =
        "local S = {}\nlocal Middleware = { Buy = function(self, p) return true end }\nS.Middleware = Middleware\nreturn S\n";
    assert_eq!(name_lines(&colon(assigned)), ["Buy:2"]);
    assert_match(&fix_all(assigned), r"Buy = function\(p\)");
}

#[test]
fn middleware_calls_and_other_colon_methods_are_not_flagged() {
    let src = lines(&[
        "local S = {}",
        "function S:Middleware(p) end",
        "function S.Client:Buy(p) end",
        "local ok = S.Middleware[name](player, ...)",
        "local ok2 = S.Middleware.Buy(player)",
        "function Other.Middleware_Thing:X(p) end",
        "return S",
    ]);
    assert!(colon(&src).is_empty());
}

// Service checks ------------------------------------------------------------------------------------

#[test]
fn reserved_client_names_orphan_middleware_and_spec_keys_are_warnings() {
    let src = lines(&[
        "local Shop = {",
        "\tClient = {",
        "\t\tSignals = function() end,",
        "\t\tServer = nil,",
        "\t},",
        "\tClientEvents = { \"Pinged\" },",
        "\tSignals = { \"Bought\" },",
        "\tMiddleware = { Pinged = function(p) end, Bye = function(p) end },",
        "\tSpec = { Buy = Loren.Method(), Bought = Loren.Signal(), Pingd = Loren.Event() },",
        "}",
        "function Shop.Client:Buy(p) end",
        "function Shop.Client:Try(p) end",
        "function Shop.Middleware.Buyy(p) end",
        "Shop.Middleware.Buy = function(p) end",
        "return Shop",
    ]);
    let issues = scan_text(&src, "src/server/Services/Shop.luau", true, false);
    assert_eq!(
        kinds(&issues),
        [
            "reserved-name:3",
            "orphan-middleware:8",
            "orphan-spec:9",
            "reserved-name:12",
            "orphan-middleware:13"
        ]
    );
    assert!(issues
        .iter()
        .all(|i| i.severity == Severity::Warn && i.file == "src/server/Services/Shop.luau"));
    assert_match(
        &issues[0].message,
        r"Shop\.Client\.Signals uses a reserved name",
    );
    assert_match(
        &issues[2].message,
        r"Shop\.Spec\.Pingd matches no Client method, ClientEvent or Signal.*Did you mean 'Pinged'\?",
    );
    assert_match(
        &issues[4].message,
        r"Shop\.Middleware\.Buyy matches no Client method or ClientEvent.*Did you mean 'Buy'\?",
    );
}

#[test]
fn orphan_checks_are_skipped_when_the_client_table_is_built_at_run_time() {
    let src = "local API = require(script.API)\nlocal S = { Client = API, Middleware = { Buy = function(p) end } }\nreturn S";
    assert!(scan_text(src, "module.luau", true, false).is_empty());
    let events =
        "local S = { ClientEvents = EVENTS, Middleware = { Buy = function(p) end } }\nreturn S";
    assert!(scan_text(events, "module.luau", true, false).is_empty());
}

#[test]
fn spec_client_events_keys_old_looking_values_warn_2_0_values_only_warn_in_legacy_mode() {
    let reserved = |src: &str, legacy: bool| -> Vec<Issue> {
        scan_text(src, "module.luau", true, legacy)
            .into_iter()
            .filter(|i| i.kind == "reserved-key")
            .collect()
    };
    let old = "local S = { Spec = { MaxHealth = 100 }, ClientEvents = { Last = 0 } }\nreturn S";
    let found: Vec<String> = reserved(old, false)
        .iter()
        .map(|i| i.message.split(' ').next().unwrap().to_string())
        .collect();
    assert_eq!(found, ["module.Spec", "module.ClientEvents"]);

    let modern =
        "local S = { ClientEvents = { \"Say\" }, Spec = { Say = Loren.Event() } }\nreturn S";
    assert!(reserved(modern, false).is_empty());
    assert_eq!(reserved(modern, true).len(), 2);

    let empty = "local S = { Spec = {}, ClientEvents = {} }\nS.Spec = nil\nreturn S";
    assert!(scan_text(empty, "module.luau", true, true).is_empty());
    let unknown = "local S = { Spec = require(script.Spec) }\nreturn S";
    assert!(scan_text(unknown, "module.luau", true, false).is_empty());
    let statement = "local S = {}\nS.ClientEvents = { Count = 1 }\nreturn S";
    assert_eq!(
        kinds(&scan_text(statement, "module.luau", true, false)),
        ["reserved-key:2"]
    );
}

#[test]
fn service_checks_only_run_where_asked_controllers_and_shared_modules_are_skipped() {
    let src = "local C = { Client = { Try = function() end } }\nreturn C";
    assert!(scan_text(src, "module.luau", false, false).is_empty());
    assert_eq!(scan_text(src, "module.luau", true, false).len(), 1);
}

// runDoctor -------------------------------------------------------------------------------------

#[test]
fn run_doctor_on_the_1_5_1_fixture_reports_every_kind_with_file_and_line() {
    let dir = project(Some("v151"));
    let log = Log::memory();
    let res = run_doctor(
        dir.path(),
        &DoctorOptions {
            legacy: true,
            ..opts(&log)
        },
    );
    assert_eq!(res.exit_code, 1, "fixable issues remain");
    let got: Vec<String> = res
        .issues
        .iter()
        .map(|i| format!("{}:{}:{}", i.file, i.line, i.kind))
        .collect();
    assert_eq!(
        got,
        [
            "src/server/Services/ShopService.luau:10:colon-middleware",
            "src/server/Services/ShopService.luau:36:reserved-name",
            "src/server/Services/ShopService.luau:41:colon-middleware",
            "src/server/Services/ShopService.luau:45:colon-middleware",
            "src/server/Services/ShopService.luau:45:orphan-middleware",
            "src/server/Services/TradeService.luau:4:orphan-spec",
            "src/server/Services/TradeService.luau:4:reserved-key",
            "src/server/Services/TradeService.luau:15:colon-middleware",
            "src/server/Services/TradeService.luau:19:colon-middleware",
        ]
    );
    assert!(res
        .issues
        .iter()
        .all(|i| matches!(i.severity, Severity::Warn | Severity::Fixable)));
    assert_match(&log.output(), r"Run `loren doctor --fix`");
    assert!(res.fixed.is_empty());
}

#[test]
fn run_doctor_fix_without_a_tty_or_yes_exits_2_and_changes_nothing() {
    let dir = project(Some("v151"));
    let before = snapshot(dir.path());
    let asked = Shared::default();
    let prompt = prompter(false, "y\n", &asked);
    let log = Log::memory();
    let res = run_doctor(
        dir.path(),
        &DoctorOptions {
            fix: true,
            prompt: Some(&prompt),
            ..opts(&log)
        },
    );
    assert_eq!(res.exit_code, 2);
    assert!(asked_text(&asked).is_empty(), "never prompts without a TTY");
    assert_match(&log.stderr(), r"needs confirmation\. Run again with --yes");
    assert_eq!(snapshot(dir.path()), before);
}

#[test]
fn run_doctor_fix_yes_rewrites_backs_up_and_re_scans_clean_manual_cases_stay() {
    let dir = project(Some("v151"));
    let root = dir.path();
    let shop_before = read(&root.join("src/server/Services/ShopService.luau"));
    let log = Log::memory();
    let res = run_doctor(
        root,
        &DoctorOptions {
            fix: true,
            yes: true,
            now: Some(now()),
            ..opts(&log)
        },
    );
    assert_eq!(res.fixed.len(), 4);
    assert_eq!(
        res.exit_code, 1,
        "TradeService.Middleware:Offer uses self: left for a hand fix"
    );
    let left: Vec<String> = res
        .remaining
        .iter()
        .filter(|i| i.severity == Severity::Fixable)
        .map(name_of)
        .collect();
    assert_eq!(left, ["Offer"]);
    assert_eq!(
        read(&root.join(format!(
            ".loren-backup/{STAMP}/src/server/Services/ShopService.luau"
        ))),
        shop_before
    );
    let shop = read(&root.join("src/server/Services/ShopService.luau"));
    assert_match(&shop, r"\t\tGift = function\(player, target\)\n");
    assert_match(
        &shop,
        r"function ShopService\.Middleware\.Buy\(player, itemId\)",
    );
    assert_match(&shop, r"function ShopService\.Middleware\n\t\.Refund\(");
    assert_match(&shop, r"check = function\(self\)");
    assert_match(
        &shop,
        r"function ShopService\.Middleware:Commented\(player\)",
    );
    assert_match(
        &shop,
        r#""function ShopService\.Middleware:InAString\(player\)""#,
    );
    let trade = read(&root.join("src/server/Services/TradeService.luau"));
    assert_match(
        &trade,
        r"TradeService\.Middleware\.Accept = function\(player, id\)",
    );
    assert_match(
        &trade,
        r"function TradeService\.Middleware:Offer\(player, offer\)",
    );
    assert_match(
        &trade,
        r"TradeService\.Client\.Offer = function\(self, player, offer\)",
    );

    let again = run_doctor(root, &opts(&Log::memory()));
    let names: Vec<String> = again
        .issues
        .iter()
        .filter(|i| i.kind == "colon-middleware")
        .map(name_of)
        .collect();
    assert_eq!(names, ["Offer"]);
}

#[test]
fn run_doctor_exit_0_once_only_warnings_remain_tty_confirm_yes_no() {
    let dir = project(Some("v151"));
    let root = dir.path();
    fs::remove_file(root.join("src/server/Services/TradeService.luau")).unwrap();
    let before = snapshot(root);

    let asked = Shared::default();
    let no = prompter(true, "n\n", &asked);
    let declined = run_doctor(
        root,
        &DoctorOptions {
            fix: true,
            is_tty: true,
            prompt: Some(&no),
            ..opts(&Log::memory())
        },
    );
    assert_match(
        &asked_text(&asked),
        r"Rewrite 3 middleware functions to dot style\? The originals are backed up to \.loren-backup/\.",
    );
    assert_eq!(declined.exit_code, 1);
    assert_eq!(snapshot(root), before);

    let yes = prompter(true, "y\n", &Shared::default());
    let accepted = run_doctor(
        root,
        &DoctorOptions {
            fix: true,
            is_tty: true,
            prompt: Some(&yes),
            now: Some(now()),
            ..opts(&Log::memory())
        },
    );
    assert_eq!(accepted.exit_code, 0, "only warnings remain");
    assert_eq!(accepted.fixed.len(), 3);
    assert!(accepted
        .remaining
        .iter()
        .all(|i| i.severity == Severity::Warn));
}

#[test]
fn run_doctor_a_clean_project_exits_0_spec_style_2_x_services_raise_nothing() {
    let dir = project(Some("v2"));
    let log = Log::memory();
    let res = run_doctor(
        dir.path(),
        &DoctorOptions {
            fix: true,
            ..opts(&log)
        },
    );
    assert!(res.issues.is_empty());
    assert_eq!(res.exit_code, 0);
    assert_match(&log.output(), "no issues");
    let empty = run_doctor(project(None).path(), &opts(&Log::memory()));
    assert_eq!(empty.exit_code, 0);
}

#[test]
fn run_doctor_only_suggests_fix_when_something_is_auto_fixable() {
    let dir = project(None);
    let root = dir.path();
    write(
        root,
        "src/server/Services/S.luau",
        "local S = {}\nfunction S.Middleware:A(p)\n\treturn self:Check(p)\nend\nreturn S\n",
    );
    let log = Log::memory();
    let res = run_doctor(root, &opts(&log));
    assert_eq!(res.exit_code, 1);
    assert!(!matches(&log.output(), r"loren doctor --fix"));
    assert_match(
        &log.output(),
        r"Rewrite the \[fix by hand\] middleware to dot style yourself",
    );

    write(
        root,
        "src/server/Services/T.luau",
        "local T = {}\nfunction T.Middleware:B(p) return true end\nreturn T\n",
    );
    let both = Log::memory();
    run_doctor(root, &opts(&both));
    assert_match(&both.output(), r"Run `loren doctor --fix`");
}

#[test]
fn run_doctor_keeps_the_first_backup_when_a_file_is_backed_up_twice_in_one_stamp() {
    let dir = project(None);
    let root = dir.path();
    write(
        root,
        "src/server/Services/S.luau",
        "local S = {}\nfunction S.Middleware:A(p) end\nreturn S\n",
    );
    let backup = format!(".loren-backup/{STAMP}/src/server/Services/S.luau");
    write(root, &backup, "older");
    run_doctor(
        root,
        &DoctorOptions {
            fix: true,
            yes: true,
            now: Some(now()),
            ..opts(&Log::memory())
        },
    );
    assert_eq!(read(&root.join(&backup)), "older");
    assert_match(
        &read(&root.join("src/server/Services/S.luau")),
        r"S\.Middleware\.A\(p\)",
    );
}

#[test]
fn run_doctor_follows_default_project_json_when_the_code_is_not_under_src() {
    let dir = project(None);
    let root = dir.path();
    write(
        root,
        "default.project.json",
        r#"{"name":"x","tree":{"$className":"DataModel","ServerScriptService":{"Server":{"$path":"game/server"}}}}"#,
    );
    write(
        root,
        "game/server/Services/S.luau",
        "local S = { Client = { Try = function() end } }\nfunction S.Middleware:A(p) end\nreturn S\n",
    );
    write(
        root,
        "src/client/C.luau",
        "local C = {}\nfunction C.Middleware:B(p) end\nreturn C\n",
    );
    let res = run_doctor(root, &opts(&Log::memory()));
    let got: Vec<String> = res
        .issues
        .iter()
        .map(|i| format!("{}:{}", i.file, i.kind))
        .collect();
    assert_eq!(
        got,
        [
            "src/client/C.luau:colon-middleware",
            "game/server/Services/S.luau:reserved-name",
            "game/server/Services/S.luau:colon-middleware",
            "game/server/Services/S.luau:orphan-middleware",
        ],
        "src/ first, then the mapped folders; Service checks only under the server folder"
    );
}

#[test]
fn run_doctor_warns_about_a_name_two_siblings_share_with_file_and_line() {
    let dir = project(None);
    let root = dir.path();
    let tree = |world: &str| {
        format!(
            r#"{{"name":"t","tree":{{"$className":"DataModel","Workspace":{{"$className":"Workspace",{world}}},"ServerScriptService":{{"Server":{{"$path":"src/server"}}}}}}}}"#
        )
    };
    write(
        root,
        "src/server/Main.server.luau",
        "local ok = 1\nlocal a = workspace.Part\n",
    );
    let dupes = |res: &rbx_loren::ctx::DoctorResult| {
        res.issues
            .iter()
            .filter(|i| i.kind == "duplicate-name")
            .cloned()
            .collect::<Vec<_>>()
    };

    write(
        root,
        "default.project.json",
        &tree(r#""Part":{"$className":"Part"}"#),
    );
    assert!(dupes(&run_doctor(root, &opts(&Log::memory()))).is_empty());

    write(
        root,
        "default.project.json",
        &tree(
            r#""Part":{"$className":"Part"},"Other":{"$className":"Folder","Part":{"$className":"Part"}}"#,
        ),
    );
    assert!(dupes(&run_doctor(root, &opts(&Log::memory()))).is_empty());

    write(
        root,
        "default.project.json",
        &tree(r#""World":{"$path":"src/world"}"#),
    );
    write(
        root,
        "src/server/Main.server.luau",
        "local ok = 1\nlocal a = workspace.World.Part\n",
    );
    write(root, "src/world/Part.server.luau", "");
    write(root, "src/world/Part.client.luau", "");
    let log = Log::memory();
    let res = run_doctor(root, &opts(&log));
    let found = dupes(&res);
    assert_eq!(found.len(), 1, "{:?}", res.issues);
    assert_eq!(found[0].file, "src/server/Main.server.luau");
    assert_eq!(found[0].line, 2);
    assert_eq!(found[0].severity, Severity::Warn);
    assert_eq!(res.exit_code, 0);
    assert_match(
        &log.output(),
        r"src/server/Main\.server\.luau:2 .*World has 2 children named",
    );
}
