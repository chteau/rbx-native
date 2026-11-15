//! Port of upstream's `test/update.test.js`. Left out because they only make
//! sense for Node: none of this file's upstream tests exercise update-notifier,
//! the Node version floor, npm pack or Windows `.cmd` shims; the last upstream
//! test's `rojo build` half needs a real Rojo, so only its file checks run.

mod common;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use common::{
    fixtures_dir, list_files, now, package_dir, project, read, template_dir, tmp_dir, FakeRunner,
    STAMP,
};
use rbx_loren::constants::SHIM_SOURCE;
use rbx_loren::ctx::{Tool, UpdateOptions, UpdateResult};
use rbx_loren::fsutil::write_text;
use rbx_loren::json::Json;
use rbx_loren::lint::{project_layout, Layout};
use rbx_loren::log::Log;
use rbx_loren::prompt::Prompter;
use rbx_loren::run::RunResult;
use rbx_loren::update::{
    classify_loren_file, compare_versions, patch_project, run_update, strip_native, Patched,
};
use regex::Regex;

const GLOBS: [&str; 2] = [
    "loren_packages/**/*.spec.lua",
    "loren_packages/**/*.spec.luau",
];

fn backup() -> String {
    format!(".loren-backup/{STAMP}")
}

fn at(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs)
}

fn matches(text: &str, pattern: &str) -> bool {
    Regex::new(pattern).unwrap().is_match(text)
}

fn assert_match(text: &str, pattern: &str) {
    assert!(matches(text, pattern), "expected /{pattern}/ in:\n{text}");
}

fn snapshot(dir: &Path) -> BTreeMap<String, String> {
    if !dir.is_dir() {
        return BTreeMap::new();
    }
    list_files(dir)
        .into_iter()
        .map(|f| {
            let text = String::from_utf8_lossy(&fs::read(dir.join(&f)).unwrap()).into_owned();
            (f, text)
        })
        .collect()
}

fn rd(dir: &Path, rel: &str) -> String {
    read(&dir.join(rel))
}

fn wr(dir: &Path, rel: &str, text: &str) {
    write_text(&dir.join(rel), text).unwrap();
}

fn ex(dir: &Path, rel: &str) -> bool {
    dir.join(rel).exists()
}

fn parse(text: &str) -> Json {
    Json::parse(text).unwrap()
}

fn at_path<'a>(json: &'a Json, path: &[&str]) -> &'a Json {
    path.iter().fold(json, |j, k| {
        j.get(k).unwrap_or_else(|| panic!("missing {k}"))
    })
}

fn keys(json: &Json) -> Vec<String> {
    match json {
        Json::Object(pairs) => pairs.iter().map(|(k, _)| k.clone()).collect(),
        other => panic!("not an object: {other:?}"),
    }
}

#[derive(Clone)]
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

struct O {
    yes: bool,
    tty: bool,
    dry_run: bool,
    no_native: bool,
    force: bool,
    tool: Option<&'static str>,
    now: Option<SystemTime>,
    runner: Option<FakeRunner>,
    answers: &'static str,
    template: Option<PathBuf>,
}

impl Default for O {
    fn default() -> Self {
        Self {
            yes: true,
            tty: false,
            dry_run: false,
            no_native: false,
            force: false,
            tool: None,
            now: Some(now()),
            runner: None,
            answers: "",
            template: None,
        }
    }
}

struct Out {
    res: UpdateResult,
    log: Log,
    runner: FakeRunner,
    asked: String,
}

fn update(dir: &Path, o: O) -> Out {
    let log = Log::memory();
    let runner = o.runner.unwrap_or_default();
    let buf = Shared(Rc::default());
    let prompt = Prompter::new(
        o.yes,
        o.tty,
        Box::new(Cursor::new(o.answers.as_bytes().to_vec())),
        Box::new(buf.clone()),
    );
    let res = run_update(
        dir,
        &UpdateOptions {
            template_dir: o.template.unwrap_or_else(package_dir),
            tool: o.tool.map(String::from),
            dry_run: o.dry_run,
            yes: o.yes,
            no_native: o.no_native,
            is_tty: o.tty,
            prompt: Some(&prompt),
            log: &log,
            runner: &runner,
            now: o.now,
            force: o.force,
        },
    );
    let asked = String::from_utf8_lossy(&buf.0.borrow()).into_owned();
    Out {
        res,
        log,
        runner,
        asked,
    }
}

fn kinds(res: &UpdateResult) -> Vec<&'static str> {
    res.actions.iter().map(|a| a.kind).collect()
}

fn calls(runner: &FakeRunner) -> Vec<String> {
    runner
        .calls()
        .iter()
        .map(|c| format!("{} {}", c.cmd, c.args.join(" ")).trim().to_string())
        .collect()
}

fn package_runtime() -> BTreeMap<String, String> {
    snapshot(&package_dir().join("loren"))
}

// TradeService has the one middleware that needs a hand rewrite (exit 1); without it an update ends at 0.
fn without_trade(dir: &Path, src: &str) {
    fs::remove_file(dir.join(src).join("server/Services/TradeService.luau")).unwrap();
}

// 1.5.1 -------------------------------------------------------------------------------------------

#[test]
fn v151_to_2_0_runtime_shim_project_promise_types_sourcemap_lint() {
    let dir = project(Some("v151"));
    let d = dir.path();
    let old_project = rd(d, "default.project.json");
    let old_runtime = rd(d, "src/shared/Loren.luau");
    let Out {
        res, log, runner, ..
    } = update(d, O::default());

    assert_eq!(
        kinds(&res),
        ["runtime", "shim", "project", "promise", "types"]
    );
    assert_eq!(res.kind, Some("legacy"));
    assert_eq!(res.from_version.as_deref(), Some("1.5.1"));
    assert_eq!(res.to_version.as_deref(), Some("2.0.0"));
    assert_eq!(
        res.exit_code, 1,
        "TradeService.Middleware:Offer needs a hand rewrite"
    );

    assert_eq!(snapshot(&d.join("loren")), package_runtime());
    assert_eq!(rd(d, "src/shared/Loren.luau"), SHIM_SOURCE);
    let project = parse(&rd(d, "default.project.json"));
    assert_eq!(
        at_path(&project, &["globIgnorePaths"]),
        &parse(&format!("[\"{}\",\"{}\"]", GLOBS[0], GLOBS[1]))
    );
    assert_eq!(keys(&project), ["name", "globIgnorePaths", "tree"]);
    assert_eq!(
        at_path(&project, &["tree", "ReplicatedStorage", "LorenRuntime"]),
        &parse(r#"{"$path":"loren/shared"}"#)
    );
    assert_eq!(
        at_path(&project, &["tree", "ServerScriptService", "LorenServer"]),
        &parse(r#"{"$path":"loren/server"}"#)
    );
    assert_eq!(
        at_path(&project, &["tree", "ReplicatedStorage", "Shared"]),
        &parse(r#"{"$path":"src/shared"}"#)
    );
    assert_eq!(
        at_path(&project, &["tree", "StarterPlayer"]),
        at_path(&parse(&old_project), &["tree", "StarterPlayer"])
    );
    assert_match(
        &rd(d, "default.project.json"),
        r#"^\{\n {4}"name": "my-game",\n {4}"globIgnorePaths": \[\n {8}"loren_packages"#,
    );
    let promise: Vec<String> = snapshot(&d.join("loren_packages/Promise"))
        .into_keys()
        .collect();
    assert_eq!(promise, ["LICENSE", "default.project.json", "lib/init.lua"]);
    assert_match(
        &rd(d, "src/shared/LorenTypes.luau"),
        r"return table\.freeze\(\{\}\)\n$",
    );
    assert!(ex(d, "src/server/LorenServerTypes.luau"));
    assert_eq!(
        calls(&runner),
        ["rojo sourcemap default.project.json -o sourcemap.json"]
    );
    assert_eq!(res.tool, Some(Tool::Rojo));
    assert_eq!(res.doctor.as_ref().unwrap().fixed.len(), 4);
    assert_match(
        &rd(d, "src/server/Services/ShopService.luau"),
        r"function ShopService\.Middleware\.Buy\(player, itemId\)",
    );
    let b = backup();
    assert_eq!(rd(d, &format!("{b}/src/shared/Loren.luau")), old_runtime);
    assert_eq!(rd(d, &format!("{b}/default.project.json")), old_project);
    assert!(ex(
        d,
        &format!("{b}/loren_packages/Promise/lib/init.spec.lua")
    ));
    assert_match(
        &rd(d, &format!("{b}/src/shared/LorenTypes.luau")),
        "return nil",
    );
    assert!(
        ex(d, &format!("{b}/src/server/Services/ShopService.luau")),
        "the lint backs up into the same folder"
    );
    assert_eq!(res.backup_dir.as_deref(), Some(b.as_str()));
    let out = log.output();
    assert_match(
        &out,
        r"Loren 1\.5\.1 project \(src/shared/Loren\.luau is the whole 1\.x runtime\)",
    );
    assert_match(&out, "To update the Loren CLI itself: update RbxNative");
}

#[test]
fn v151_without_hand_fixes_left_exits_0_and_a_second_run_changes_nothing() {
    let dir = project(Some("v151"));
    let d = dir.path();
    without_trade(d, "src");
    let first = update(d, O::default());
    assert_eq!(first.res.exit_code, 0);
    let after = snapshot(d);
    let second = update(
        d,
        O {
            now: Some(at(1_790_294_400)),
            ..O::default()
        },
    );
    assert_eq!(second.res.exit_code, 0);
    assert!(second.res.actions.is_empty());
    assert_eq!(second.res.kind, Some("v2"));
    assert_eq!(snapshot(d), after, "nothing changed, no new backup");
    assert_match(&second.log.output(), r"Already up to date: runtime 2\.0\.0");
    assert_eq!(second.runner.calls().len(), 1, "the sourcemap still runs");
}

#[test]
fn no_tty_and_no_yes_exit_2_nothing_changed_never_prompts() {
    let dir = project(Some("v151"));
    let d = dir.path();
    let before = snapshot(d);
    let Out {
        res,
        log,
        runner,
        asked,
    } = update(
        d,
        O {
            yes: false,
            answers: "y\n",
            ..O::default()
        },
    );
    assert_eq!(res.exit_code, 2);
    assert!(asked.is_empty());
    assert!(runner.calls().is_empty());
    assert_match(
        &log.stderr(),
        r"needs confirmation\. Run it again with --yes, or with --dry-run to preview\. Nothing was changed\.",
    );
    assert_eq!(snapshot(d), before);
}

#[test]
fn tty_declining_cancels_exit_1_nothing_changed_accepting_applies() {
    let dir = project(Some("v151"));
    let d = dir.path();
    without_trade(d, "src");
    let before = snapshot(d);
    let declined = update(
        d,
        O {
            yes: false,
            tty: true,
            answers: "n\n",
            ..O::default()
        },
    );
    assert_eq!(declined.res.exit_code, 1);
    assert!(declined.res.cancelled);
    assert_eq!(
        declined
            .asked
            .matches("Update this project to Loren 2.0.0?")
            .count(),
        1
    );
    assert_eq!(declined.asked.to_lowercase().matches("(y/n)").count(), 1);
    assert_eq!(snapshot(d), before);

    let accepted = update(
        d,
        O {
            yes: false,
            tty: true,
            answers: "y\ny\n",
            ..O::default()
        },
    );
    assert_eq!(accepted.res.exit_code, 0);
    assert_eq!(
        accepted.asked.to_lowercase().matches("(y/n)").count(),
        2,
        "the update and the middleware rewrite are confirmed separately"
    );
    assert_eq!(rd(d, "src/shared/Loren.luau"), SHIM_SOURCE);
}

#[test]
fn dry_run_prints_the_plan_and_the_lint_preview_and_changes_nothing() {
    let dir = project(Some("v151"));
    let d = dir.path();
    let before = snapshot(d);
    let Out {
        res, log, runner, ..
    } = update(
        d,
        O {
            dry_run: true,
            yes: false,
            ..O::default()
        },
    );
    assert_eq!(res.exit_code, 0);
    assert!(res.dry_run);
    assert_eq!(
        kinds(&res),
        ["runtime", "shim", "project", "promise", "types"]
    );
    assert!(runner.calls().is_empty());
    assert_eq!(snapshot(d), before);
    let out = log.output();
    assert_match(&out, "Would do:");
    assert_match(&out, r"add loren/ with runtime 2\.0\.0 \(4 files\)");
    assert_match(
        &out,
        r"write the shim to src/shared/Loren\.luau \(replaces the 1\.5\.1 runtime\)",
    );
    assert_match(
        &out,
        r"globIgnorePaths \+= loren_packages/\*\*/\*\.spec\.lua, loren_packages/\*\*/\*\.spec\.luau",
    );
    assert_match(
        &out,
        r"it has test files \(lib/init\.spec\.lua\) that would build into your place",
    );
    assert_match(&out, r"ShopService\.luau:41 \[fixable\]");
    assert_match(&out, "Dry run: nothing was changed");
}

// 2.x -----------------------------------------------------------------------------------------------

#[test]
fn v2_replaces_loren_without_stale_files_keeps_the_rest_of_the_project_file() {
    let dir = project(Some("v2"));
    let d = dir.path();
    let old_project = parse(&rd(d, "default.project.json"));
    let Out { res, log, .. } = update(d, O::default());
    assert_eq!(res.exit_code, 0);
    assert_eq!(res.kind, Some("v2"));
    assert_eq!(res.from_version.as_deref(), Some("2.0.0-beta.1"));
    assert_eq!(kinds(&res), ["runtime", "project", "types"]);
    assert!(
        !ex(d, "loren/shared/Old.luau"),
        "stale runtime file removed"
    );
    assert!(
        ex(d, &format!("{}/loren/shared/Old.luau", backup())),
        "but backed up"
    );
    assert_eq!(snapshot(&d.join("loren")), package_runtime());
    let project = parse(&rd(d, "default.project.json"));
    let expected = parse(&format!(
        r#"{{"name":"v2-game","globIgnorePaths":["{}","{}"],"servePort":34872,"tree":{}}}"#,
        GLOBS[0],
        GLOBS[1],
        at_path(&old_project, &["tree"]).stringify(0)
    ));
    assert_eq!(project, expected);
    assert_eq!(
        keys(&project),
        ["name", "globIgnorePaths", "servePort", "tree"]
    );
    assert_match(
        &log.output(),
        r"Loren 2\.0\.0-beta\.1 project \(src/shared/Loren\.luau is the 2\.x shim\)",
    );
}

#[test]
fn v2_idempotent_second_run_no_actions_no_writes_no_backup_and_still_reports() {
    let dir = project(Some("v2"));
    let d = dir.path();
    update(d, O::default());
    let after = snapshot(d);
    let again = update(
        d,
        O {
            now: Some(at(1_893_456_000)),
            ..O::default()
        },
    );
    assert_eq!(again.res.exit_code, 0);
    assert!(again.res.actions.is_empty());
    assert_eq!(snapshot(d), after);
    assert!(!ex(d, ".loren-backup/2030-01-01T00-00-00-000Z"));
    assert_match(&again.log.output(), "Already up to date");
    assert_match(&again.log.output(), "Middleware lint: no issues");
}

#[test]
fn v2_crlf_checkouts_of_an_up_to_date_project_count_as_unchanged() {
    let dir = project(Some("v2"));
    let d = dir.path();
    update(d, O::default());
    for f in [
        "loren/shared/Codec.luau",
        "src/shared/Loren.luau",
        "src/shared/LorenTypes.luau",
        "loren_packages/Promise/lib/init.lua",
    ] {
        wr(d, f, &rd(d, f).replace('\n', "\r\n"));
    }
    let again = update(
        d,
        O {
            now: Some(at(1_893_456_000)),
            ..O::default()
        },
    );
    assert!(again.res.actions.is_empty());
}

#[test]
fn no_native_strips_native_and_keeps_line_numbers_and_is_idempotent() {
    let dir = project(Some("v2"));
    let d = dir.path();
    let Out { res, .. } = update(
        d,
        O {
            no_native: true,
            ..O::default()
        },
    );
    assert_eq!(res.exit_code, 0);
    let pkg = package_runtime();
    for (file, text) in snapshot(&d.join("loren")) {
        assert!(
            !matches(&text, r"(?m)^\s*@native"),
            "{file} has no @native attribute"
        );
        assert!(!text.contains("--!native"), "{file} has no --!native");
        assert_eq!(
            text.split('\n').count(),
            pkg[&file].split('\n').count(),
            "{file} keeps its line count"
        );
    }
    let codec = rd(d, "loren/shared/Codec.luau");
    assert_match(&codec, r"\nlocal function mul\(");
    assert_match(&codec, "-- a comment that mentions @native stays");
    assert!(update(
        d,
        O {
            no_native: true,
            ..O::default()
        }
    )
    .res
    .actions
    .is_empty());
    assert_eq!(
        kinds(&update(d, O::default()).res),
        ["runtime"],
        "without the flag @native comes back"
    );
}

#[test]
fn strip_native_on_crlf_text() {
    assert_eq!(
        strip_native("--!native\r\n@native\r\nlocal x = 1\r\n\t@native local function f() end\r\n"),
        "\r\n\r\nlocal x = 1\r\n\tlocal function f() end\r\n"
    );
}

#[test]
fn argon_project_the_sourcemap_uses_argon_tool_overrides_detection() {
    let dir = project(Some("argon"));
    let Out { res, runner, .. } = update(dir.path(), O::default());
    assert_eq!(res.exit_code, 0);
    assert_eq!(res.tool, Some(Tool::Argon));
    assert_eq!(
        calls(&runner),
        ["argon sourcemap default.project.json -o sourcemap.json"]
    );

    let rojo_project = project(Some("v2"));
    let forced = update(
        rojo_project.path(),
        O {
            tool: Some("argon"),
            ..O::default()
        },
    );
    let cmds: Vec<String> = forced
        .runner
        .calls()
        .iter()
        .map(|c| c.cmd.clone())
        .collect();
    assert_eq!(cmds, ["argon"]);
    assert_eq!(forced.res.tool, Some(Tool::Argon));
}

#[test]
fn a_failing_or_missing_sync_tool_files_are_updated_exit_1_with_a_clear_message() {
    let dir = project(Some("v2"));
    let d = dir.path();
    let failing_runner = FakeRunner::new().handler("rojo", |args, _| {
        (args.first().map(String::as_str) == Some("sourcemap")).then(|| RunResult {
            status: Some(1),
            stderr: "boom".into(),
            ..RunResult::default()
        })
    });
    let failing = update(
        d,
        O {
            runner: Some(failing_runner),
            ..O::default()
        },
    );
    assert_eq!(failing.res.exit_code, 1);
    assert_eq!(rd(d, "src/shared/Loren.luau"), SHIM_SOURCE);
    assert_match(
        &failing.log.stderr(),
        r"sourcemap\.json was not regenerated: Rojo sourcemap failed \(exit 1\): boom\. If rokit\.toml pins a version you do not have, install it first \(rokit install\)\.",
    );

    let bare = project(Some("v2"));
    let b = bare.path();
    fs::remove_file(b.join("rokit.toml")).unwrap();
    let nothing = FakeRunner::new()
        .missing("rojo")
        .missing("argon")
        .missing("rokit")
        .missing("aftman")
        .missing("foreman");
    let none = update(
        b,
        O {
            runner: Some(nothing),
            ..O::default()
        },
    );
    assert_eq!(none.res.exit_code, 1);
    assert!(ex(b, "loren/shared/Codec.luau"));
    assert_match(&none.log.stderr(), "No sync tool found");
}

#[test]
fn an_unknown_tool_fails_before_anything_changes() {
    let dir = project(Some("v2"));
    let d = dir.path();
    let before = snapshot(d);
    let Out {
        res, log, runner, ..
    } = update(
        d,
        O {
            tool: Some("wally"),
            ..O::default()
        },
    );
    assert_eq!(res.exit_code, 1);
    assert!(res.actions.is_empty());
    assert!(runner.calls().is_empty());
    assert_match(
        &log.stderr(),
        r#"Unknown sync tool "wally"\. Use --tool rojo or --tool argon\. Nothing was changed\."#,
    );
    assert_eq!(snapshot(d), before);
}

#[test]
fn with_the_real_clock_the_update_and_the_lint_rewrite_back_up_into_one_folder() {
    let dir = project(Some("v151"));
    let d = dir.path();
    without_trade(d, "src");
    let Out { res, .. } = update(
        d,
        O {
            now: None,
            ..O::default()
        },
    );
    assert_eq!(res.exit_code, 0);
    assert_eq!(res.doctor.as_ref().unwrap().fixed.len(), 3);
    let stamps: Vec<String> = fs::read_dir(d.join(".loren-backup"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(stamps.len(), 1, "{}", stamps.join(", "));
    assert_eq!(res.backup_dir, Some(format!(".loren-backup/{}", stamps[0])));
    assert!(ex(
        d,
        &format!(
            ".loren-backup/{}/src/server/Services/ShopService.luau",
            stamps[0]
        )
    ));
    assert!(ex(
        d,
        &format!(".loren-backup/{}/src/shared/Loren.luau", stamps[0])
    ));
}

#[test]
fn dotfiles_in_loren_do_not_make_an_up_to_date_runtime_differ() {
    let dir = project(Some("v2"));
    let d = dir.path();
    update(d, O::default());
    wr(d, "loren/shared/.DS_Store", "finder");
    let again = update(
        d,
        O {
            now: Some(at(1_893_456_000)),
            ..O::default()
        },
    );
    assert!(again.res.actions.is_empty());
    wr(d, "loren/shared/Stale.luau", "return nil");
    let stale = update(
        d,
        O {
            now: Some(at(1_893_542_400)),
            ..O::default()
        },
    );
    assert_eq!(kinds(&stale.res), ["runtime"]);
    assert!(!ex(d, "loren/shared/Stale.luau") && !ex(d, "loren/shared/.DS_Store"));
}

#[test]
fn a_project_runtime_newer_than_the_cli_is_refused() {
    let dir = project(Some("v2"));
    let d = dir.path();
    wr(
        d,
        "loren/shared/Version.luau",
        "return table.freeze({ Version = \"2.1.0\" })\n",
    );
    let before = snapshot(d);
    let Out { res, log, .. } = update(d, O::default());
    assert_eq!(res.exit_code, 1);
    assert_match(
        &log.stderr(),
        r"newer than this CLI's \(2\.0\.0\)\. Update the CLI first: update RbxNative",
    );
    assert_eq!(snapshot(d), before);
    assert_eq!(
        update(
            d,
            O {
                force: true,
                ..O::default()
            }
        )
        .res
        .exit_code,
        0
    );
}

#[test]
fn promise_missing_is_vendored_already_minimal_untouched() {
    let dir = project(Some("v2"));
    let d = dir.path();
    fs::remove_dir_all(d.join("loren_packages/Promise")).unwrap();
    let Out { res, .. } = update(d, O::default());
    assert!(res
        .actions
        .iter()
        .any(|a| a.kind == "promise" && a.detail.contains("it is missing")));
    assert_eq!(
        rd(d, "loren_packages/Promise/lib/init.lua"),
        read(&package_dir().join("loren_packages/Promise/lib/init.lua"))
    );
    assert!(!ex(d, "loren_packages/Promise/lib/init.spec.lua"));
    assert!(!ex(d, "loren_packages/Promise/README.md"));
}

#[test]
fn the_shim_a_changed_shim_and_a_stray_loren_lua_are_replaced_both_backed_up() {
    let dir = project(Some("v2"));
    let d = dir.path();
    wr(
        d,
        "src/shared/Loren.luau",
        &format!("-- my edit\n{SHIM_SOURCE}"),
    );
    let edited = update(d, O::default());
    assert!(edited
        .res
        .actions
        .iter()
        .any(|a| a.kind == "shim" && a.detail.contains("a changed shim")));
    assert_eq!(rd(d, "src/shared/Loren.luau"), SHIM_SOURCE);

    let legacy = project(Some("v151"));
    let l = legacy.path();
    fs::rename(
        l.join("src/shared/Loren.luau"),
        l.join("src/shared/Loren.lua"),
    )
    .unwrap();
    let res = update(l, O::default());
    assert_eq!(res.res.kind, Some("legacy"));
    assert!(!ex(l, "src/shared/Loren.lua"));
    assert_eq!(rd(l, "src/shared/Loren.luau"), SHIM_SOURCE);
    assert!(ex(l, &format!("{}/src/shared/Loren.lua", backup())));
}

// Not a Loren project ---------------------------------------------------------------------------------

#[test]
fn not_a_loren_project_exit_1_with_a_clear_message_nothing_written() {
    let empty = project(None);
    let a = update(empty.path(), O::default());
    assert_eq!(a.res.exit_code, 1);
    assert_match(
        &a.log.stderr(),
        r"is not a Loren project: there is no default\.project\.json",
    );
    assert!(snapshot(empty.path()).is_empty());

    let other = project(None);
    let o = other.path();
    wr(
        o,
        "default.project.json",
        r#"{ "name": "x", "tree": { "$className": "DataModel" } }"#,
    );
    let b = update(o, O::default());
    assert_eq!(b.res.exit_code, 1);
    assert_match(
        &b.log.stderr(),
        r"is not a Loren project: there is no src/shared/Loren\.luau and no loren/ runtime\. Nothing was changed\.",
    );

    wr(o, "src/shared/Loren.luau", "return \"my own module\"\n");
    let c = update(o, O::default());
    assert_eq!(c.res.exit_code, 1);
    assert_match(
        &c.log.stderr(),
        r"src/shared/Loren\.luau is neither the Loren 1\.x runtime nor the 2\.x shim",
    );

    let broken = project(Some("v2"));
    wr(broken.path(), "default.project.json", "{ \"name\": ");
    let d = update(broken.path(), O::default());
    assert_eq!(d.res.exit_code, 1);
    assert_match(&d.log.stderr(), r"default\.project\.json is not valid JSON");
}

#[test]
fn an_incomplete_cli_install_is_reported() {
    let pkg = tmp_dir();
    let dir = project(Some("v2"));
    let Out { res, log, .. } = update(
        dir.path(),
        O {
            template: Some(pkg.path().to_path_buf()),
            ..O::default()
        },
    );
    assert_eq!(res.exit_code, 1);
    assert_match(&log.stderr(), "This install of the Loren CLI is incomplete");
}

// default.project.json ------------------------------------------------------------------------------

fn layout() -> Layout {
    Layout {
        shared: "src/shared".into(),
        server: "src/server".into(),
        client: "src/client".into(),
        packages: "loren_packages".into(),
        runtime_shared: None,
        runtime_server: None,
    }
}

fn patched(input: &Json) -> (Json, Vec<String>) {
    match patch_project(input, &layout()) {
        Patched::Ok { data, changes } => (data, changes),
        Patched::Error { error, .. } => panic!("unexpected error: {error}"),
    }
}

#[test]
fn patch_project_creates_missing_services_with_class_name() {
    let (data, changes) = patched(&parse(
        r#"{"name":"x","tree":{"$className":"DataModel","Workspace":{}}}"#,
    ));
    assert_eq!(
        at_path(&data, &["tree", "ReplicatedStorage"]),
        &parse(
            r#"{"$className":"ReplicatedStorage","LorenRuntime":{"$path":"loren/shared"},"LorenPackages":{"$path":"loren_packages"}}"#
        )
    );
    assert_eq!(
        at_path(&data, &["tree", "ServerScriptService"]),
        &parse(r#"{"$className":"ServerScriptService","LorenServer":{"$path":"loren/server"}}"#)
    );
    assert_eq!(keys(&data), ["name", "globIgnorePaths", "tree"]);
    assert!(changes.contains(&"added ReplicatedStorage".to_string()));
}

#[test]
fn patch_project_odd_but_valid_trees_are_patched_in_place() {
    let input = parse(
        r#"{
        "tree": {
            "$className": "DataModel",
            "ReplicatedStorage": { "$path": "src/ReplicatedStorage", "LorenRuntime": { "$path": "vendor/loren", "$ignoreUnknownInstances": true } },
            "ServerScriptService": { "$path": { "optional": "src/sss" } }
        },
        "globIgnorePaths": ["**/*.tmp", "loren_packages/**/*.spec.lua"],
        "emitLegacyScripts": false
    }"#,
    );
    let (data, changes) = patched(&input);
    assert_eq!(
        at_path(&data, &["tree", "ReplicatedStorage", "LorenRuntime"]),
        &parse(r#"{"$path":"loren/shared","$ignoreUnknownInstances":true}"#)
    );
    assert!(changes.contains(
        &"ReplicatedStorage.LorenRuntime now points at loren/shared (was vendor/loren)".to_string()
    ));
    assert_eq!(
        at_path(&data, &["tree", "ReplicatedStorage", "$path"]).as_str(),
        Some("src/ReplicatedStorage")
    );
    assert_eq!(
        at_path(&data, &["tree", "ServerScriptService", "LorenServer"]),
        &parse(r#"{"$path":"loren/server"}"#)
    );
    assert_eq!(
        at_path(&data, &["globIgnorePaths"]),
        &parse(r#"["**/*.tmp","loren_packages/**/*.spec.lua","loren_packages/**/*.spec.luau"]"#)
    );
    assert_eq!(at_path(&data, &["emitLegacyScripts"]), &parse("false"));
    assert_eq!(
        keys(&data),
        ["tree", "globIgnorePaths", "emitLegacyScripts"]
    );
    assert_eq!(
        at_path(
            &input,
            &["tree", "ReplicatedStorage", "LorenRuntime", "$path"]
        )
        .as_str(),
        Some("vendor/loren"),
        "the input is not mutated"
    );
}

#[test]
fn patch_project_a_class_name_on_loren_runtime_and_server_is_dropped() {
    let input = parse(
        r#"{"tree":{"$className":"DataModel",
        "ReplicatedStorage":{"LorenRuntime":{"$className":"Folder"}},
        "ServerScriptService":{"LorenServer":{"$className":"Folder","$path":"loren/server","Extra":{"$path":"x"}}}}}"#,
    );
    let (data, changes) = patched(&input);
    assert_eq!(
        at_path(&data, &["tree", "ReplicatedStorage", "LorenRuntime"]),
        &parse(r#"{"$path":"loren/shared"}"#)
    );
    assert_eq!(
        at_path(&data, &["tree", "ServerScriptService", "LorenServer"]),
        &parse(r#"{"$path":"loren/server","Extra":{"$path":"x"}}"#)
    );
    assert!(changes.contains(
        &"ServerScriptService.LorenServer: removed \"$className\": \"Folder\" (it conflicts with $path)".to_string()
    ));
    assert!(
        patched(&data).1.is_empty(),
        "idempotent: patching the result changes nothing more"
    );
}

#[test]
fn patch_project_trees_it_cannot_patch_safely_are_refused_with_the_lines_to_add_by_hand() {
    let cases = [
        ("[]", "is not a JSON object"),
        (r#"{"name":"x"}"#, "has no \"tree\" object"),
        (
            r#"{"tree":{"$className":"Folder","$path":"src"}}"#,
            r#"does not describe a place: its tree's \$className is "Folder""#,
        ),
        (
            r#"{"tree":{"$path":"src"}}"#,
            r"its tree's \$className is null",
        ),
        (
            r#"{"tree":{"$className":"DataModel","ReplicatedStorage":"x"}}"#,
            r"tree\.ReplicatedStorage .* is not an object",
        ),
        (
            r#"{"tree":{"$className":"DataModel","ServerScriptService":{"LorenServer":3}}}"#,
            r"LorenServer .* is not an object",
        ),
        (
            r#"{"globIgnorePaths":"**/*.spec.lua","tree":{"$className":"DataModel"}}"#,
            r#""globIgnorePaths" .* is not a list"#,
        ),
    ];
    for (input, re) in cases {
        match patch_project(&parse(input), &layout()) {
            Patched::Error { error, manual } => {
                assert_match(&error, re);
                assert_match(
                    &manual,
                    r#"tree\.ReplicatedStorage\.LorenRuntime = \{ "\$path": "loren/shared" \}"#,
                );
            }
            Patched::Ok { .. } => panic!("{input} should be refused"),
        }
    }
}

#[test]
fn an_unpatchable_tree_stops_the_update_before_anything_changes() {
    let dir = project(Some("v151"));
    let d = dir.path();
    wr(
        d,
        "default.project.json",
        r#"{"name":"model","tree":{"$className":"Folder","$path":"src"}}"#,
    );
    let before = snapshot(d);
    let Out { res, log, .. } = update(d, O::default());
    assert_eq!(res.exit_code, 1);
    assert_match(
        &log.stderr(),
        "does not describe a place.*nothing was changed",
    );
    assert_match(&log.stdout(), "Add these by hand:");
    assert_eq!(snapshot(d), before);
}

#[test]
fn custom_layout_from_the_tree_and_crlf_project_files() {
    let dir = project(Some("v151"));
    let d = dir.path();
    fs::rename(d.join("src"), d.join("game")).unwrap();
    fs::rename(d.join("loren_packages"), d.join("packages")).unwrap();
    let text = rd(d, "default.project.json")
        .replace("src/", "game/")
        .replace("\"loren_packages\"", "\"packages\"")
        .replace('\n', "\r\n");
    wr(d, "default.project.json", &text);
    without_trade(d, "game");
    let got = project_layout(d, Some(&parse(&text)));
    assert_eq!(
        (
            got.shared.as_str(),
            got.server.as_str(),
            got.client.as_str(),
            got.packages.as_str()
        ),
        ("game/shared", "game/server", "game/client", "packages")
    );
    let Out { res, .. } = update(d, O::default());
    assert_eq!(res.exit_code, 0);
    let out = rd(d, "default.project.json");
    assert!(
        out.contains("\r\n") && !matches(&out, r"[^\r]\n"),
        "CRLF kept"
    );
    assert_eq!(
        at_path(&parse(&out), &["globIgnorePaths"]),
        &parse(r#"["packages/**/*.spec.lua","packages/**/*.spec.luau"]"#)
    );
    assert_eq!(rd(d, "game/shared/Loren.luau"), SHIM_SOURCE);
    assert!(ex(d, "game/shared/LorenTypes.luau"));
    assert!(ex(d, "game/server/LorenServerTypes.luau"));
    assert!(ex(d, "packages/Promise/lib/init.lua"));
}

#[test]
fn a_tree_folder_outside_the_project_is_explained_and_the_default_folder_is_used() {
    let dir = project(Some("v2"));
    let d = dir.path();
    let mut data = parse(&rd(d, "default.project.json"));
    data.get_mut("tree")
        .unwrap()
        .get_mut("ReplicatedStorage")
        .unwrap()
        .set("LorenPackages", parse(r#"{"$path":"../shared-packages"}"#));
    wr(d, "default.project.json", &data.stringify(4));
    let Out { res, log, .. } = update(d, O::default());
    assert_eq!(res.exit_code, 0);
    assert_match(
        &log.stderr(),
        r"maps ReplicatedStorage\.LorenPackages -> \.\./shared-packages, outside this project",
    );
    let after = parse(&rd(d, "default.project.json"));
    assert_eq!(
        at_path(
            &after,
            &["tree", "ReplicatedStorage", "LorenPackages", "$path"]
        )
        .as_str(),
        Some("../shared-packages"),
        "left as it was"
    );
    assert!(
        !d.join("../shared-packages").exists(),
        "nothing written outside the project"
    );
}

// Small helpers ---------------------------------------------------------------------------------------

#[test]
fn classify_loren_file_and_compare_versions() {
    assert_eq!(classify_loren_file(Some(SHIM_SOURCE)), "shim");
    let bom = format!("\u{feff}{}", SHIM_SOURCE.replace('\n', "\r\n"));
    assert_eq!(classify_loren_file(Some(&bom)), "shim");
    assert_eq!(
        classify_loren_file(Some(&read(
            &fixtures_dir().join("v151/src/shared/Loren.luau")
        ))),
        "runtime"
    );
    assert_eq!(classify_loren_file(None), "missing");
    assert_eq!(classify_loren_file(Some("return 1")), "other");
    assert_eq!(
        read(&fixtures_dir().join("v2/src/shared/Loren.luau")).replace("\r\n", "\n"),
        SHIM_SOURCE,
        "the 2.x fixture holds the current shim"
    );
    assert_eq!(
        read(&template_dir().join("src/shared/Loren.luau")).replace("\r\n", "\n"),
        SHIM_SOURCE,
        "the scaffold shim is SHIM_SOURCE"
    );
    assert_eq!(compare_versions("2.0.0", "2.0.0"), 0);
    assert_eq!(compare_versions("2.0.0-beta.1", "2.0.0"), -1);
    assert_eq!(compare_versions("2.1.0", "2.0.9"), 1);
    assert_eq!(compare_versions("10.0.0", "9.9.9"), 1);
    assert_eq!(compare_versions("2.0.0-beta.10", "2.0.0-beta.9"), 1);
    assert_eq!(compare_versions("2.0.0-beta.2", "2.0.0-beta.10"), -1);
    assert_eq!(
        compare_versions("2.0.0-rc-2", "2.0.0-rc-1"),
        1,
        "a dash inside the pre-release is kept"
    );
    assert_eq!(compare_versions("2.0.0-beta", "2.0.0-beta.1"), -1);
    assert_eq!(
        compare_versions("2.0.0-1", "2.0.0-alpha"),
        -1,
        "numeric parts sort before words"
    );
    assert_eq!(compare_versions("2.0.0-rc.1", "2.0.0-beta.3"), 1);
    assert_eq!(compare_versions("2.0.0+build.5", "2.0.0"), 0);
}

#[test]
fn the_real_package_full_runtime_copied_no_spec_files_anywhere() {
    let dir = project(Some("v151"));
    let d = dir.path();
    without_trade(d, "src");
    fs::remove_file(d.join("aftman.toml")).unwrap();
    let Out { res, .. } = update(
        d,
        O {
            template: Some(template_dir()),
            ..O::default()
        },
    );
    assert_eq!(res.exit_code, 0);
    let real: Vec<String> = snapshot(&template_dir().join("loren"))
        .into_keys()
        .collect();
    let got: Vec<String> = snapshot(&d.join("loren")).into_keys().collect();
    assert_eq!(got, real);
    let specs: Vec<String> = snapshot(d)
        .into_keys()
        .filter(|f| !f.starts_with(".loren-backup/") && matches(f, r"\.spec\.luau?$"))
        .collect();
    assert!(specs.is_empty(), "{specs:?}");
}
