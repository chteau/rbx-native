//! Studio-only plugin APIs that reach the editor: script documents, file
//! prompts, and the requests Studio acts on itself.

use std::path::PathBuf;

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_lua::plugin::{
    Answer, Context, Input, Origin, Outcome, OutputKind, PluginHost, PluginSource, PromptKind,
    Request, SourceBody, StudioAction,
};
use rbx_reflection::ReflectionDatabase;

struct Fixture {
    host: PluginHost,
    dom: WeakDom,
    script: Ref,
    scripts: Vec<Ref>,
}

impl Fixture {
    fn new(src: &str, scripts_open: bool) -> (Fixture, Outcome) {
        let mut dom = WeakDom::new();
        dom.new_instance("Workspace", "Workspace", None);
        let storage = dom.new_instance("ServerScriptService", "ServerScriptService", None);
        let script = dom.new_instance("Script", "Main", Some(storage));
        dom.set_property(
            script,
            "Source",
            Variant::String("local a = 1\nprint(a)\n".into()),
        )
        .unwrap();
        dom.take_changes();
        let mut host = PluginHost::new(ReflectionDatabase::embedded(), None);
        // These plugins edit scripts: the user said yes to Script injection.
        let mut policy = rbx_permissions::Policy::default();
        policy.set(
            "StudioPlugin",
            rbx_lua::plugin::Capability::EditScripts,
            Some(rbx_permissions::Decision::Allow),
        );
        host.set_permissions(rbx_permissions::Store::in_memory(policy).shared());
        host.add(PluginSource {
            name: "StudioPlugin".into(),
            body: SourceBody::Luau(src.into()),
            origin: Origin::Roblox {
                path: PathBuf::from("StudioPlugin.lua"),
            },
        });
        let mut f = Fixture {
            host,
            dom,
            script,
            scripts: if scripts_open {
                vec![script]
            } else {
                Vec::new()
            },
        };
        let out = f.send(Input::Start);
        (f, out)
    }

    fn send(&mut self, input: Input) -> Outcome {
        let ctx = Context {
            now: 1.0,
            scripts: self.scripts.clone(),
            ..Context::default()
        };
        self.host.dispatch(&mut self.dom, ctx, input)
    }

    fn source(&self) -> String {
        match self
            .dom
            .get(self.script)
            .unwrap()
            .properties()
            .get("Source")
        {
            Some(Variant::String(s)) => s.clone(),
            _ => String::new(),
        }
    }
}

fn printed(out: &Outcome) -> Vec<String> {
    out.output
        .iter()
        .filter(|l| l.kind != OutputKind::Info)
        .map(|l| l.text.clone())
        .collect()
}

#[test]
fn script_documents_read_and_edit_the_open_scripts() {
    let src = r#"
        local SES = game:GetService("ScriptEditorService")
        local docs = SES:GetScriptDocuments()
        print(#docs, docs[1]:IsCommandBar(), docs[2]:IsCommandBar(), docs[2]:GetScript().Name)
        local doc = SES:FindScriptDocument(docs[2]:GetScript())
        print(doc == docs[2], doc:GetLineCount(), doc:GetLine(2), doc:GetText(1, 7, 1, 12))
        print(pcall(function() return doc:GetLine(9) end))
        print(doc:EditTextAsync("b", 1, 7, 1, 8))
        print(doc:GetLine(1))
        SES:RegisterAutocompleteCallback("x", 1, function() end)
        print(pcall(function() SES:RegisterAutocompleteCallback("x", 1, function() end) end))
        SES:DeregisterAutocompleteCallback("x")
        print(pcall(function() SES:DeregisterAutocompleteCallback("x") end))
        print(docs[1].Name, docs[1]:EditTextAsync("x", 1, 1, 1, 1))
        print(pcall(function() return docs[1]:CloseAsync() end))
    "#;
    let (f, out) = Fixture::new(src, true);
    assert_eq!(
        printed(&out),
        [
            "2 true false Main",
            "true 3 print(a) a = 1",
            "false Attempt to access a document with an index out of bounds",
            "true nil",
            "local b = 1",
            "false Already have callback with named x",
            "false No callback with name x",
            "CommandBar false Cannot apply edits from Lua to the CommandBar",
            "false Cannot close the Command Bar",
        ]
    );
    assert_eq!(f.source(), "local b = 1\nprint(a)\n");
}

#[test]
fn opening_and_closing_tabs_fire_document_events() {
    let src = r#"
        local SES = game:GetService("ScriptEditorService")
        SES.TextDocumentDidOpen:Connect(function(doc) print("open", doc:GetScript().Name) end)
        SES.TextDocumentDidClose:Connect(function(doc) print("close", doc:GetScript().Name) end)
        print(SES:FindScriptDocument(game.ServerScriptService.Main))
        SES:OpenScriptDocumentAsync(game.ServerScriptService.Main)
    "#;
    let (mut f, out) = Fixture::new(src, false);
    assert_eq!(printed(&out), ["nil"]);
    assert!(out
        .requests
        .iter()
        .any(|r| matches!(r, Request::OpenScript { script, .. } if *script == f.script)));
    f.scripts = vec![f.script];
    assert_eq!(printed(&f.send(Input::Tick)), ["open Main"]);
    f.scripts.clear();
    assert_eq!(printed(&f.send(Input::Tick)), ["close Main"]);
}

#[test]
fn import_prompt_gives_a_file() {
    let dir = std::env::temp_dir().join(format!("rbx_lua_plugin_studio_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("notes.txt");
    std::fs::write(&path, "hello").unwrap();
    let src = r#"
        local file = game:GetService("StudioService"):PromptImportFileAsync({"txt"})
        print(file.Name, file.ClassName, file.Size, file:GetBinaryContents(), typeof(file))
        print(game:GetService("StudioService"):PromptImportFileAsync({"txt"}))
    "#;
    let (mut f, out) = Fixture::new(src, false);
    let prompt = |out: &Outcome| {
        out.requests
            .iter()
            .find_map(|r| match r {
                Request::Prompt { id, kind, .. } => Some((*id, kind.clone())),
                _ => None,
            })
            .expect("a file prompt")
    };
    let (id, kind) = prompt(&out);
    assert_eq!(
        kind,
        PromptKind::OpenFiles {
            extensions: vec!["txt".into()],
            multiple: false
        }
    );
    let out = f.send(Input::PromptAnswered(id, Answer::Paths(Some(vec![path]))));
    assert_eq!(printed(&out), ["notes.txt File 5 hello Instance"]);
    let (id, _) = prompt(&out);
    let out = f.send(Input::PromptAnswered(id, Answer::Paths(None)));
    assert_eq!(printed(&out), ["nil"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn studio_actions_reach_the_editor() {
    let src = r#"
        workspace:ZoomToExtents()
        game:GetService("RunService"):Run()
        game:GetService("RunService"):Stop()
    "#;
    let (_, out) = Fixture::new(src, false);
    let actions: Vec<_> = out
        .requests
        .iter()
        .filter_map(|r| match r {
            Request::Studio(action) => Some(action.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        actions,
        [
            StudioAction::ZoomToExtents,
            StudioAction::Run,
            StudioAction::Stop
        ]
    );
}
