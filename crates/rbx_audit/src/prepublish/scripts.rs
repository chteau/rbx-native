//! `deprecated-api`, `datastore-without-pcall` and `streaming-index`: one
//! walk per script over the `full_moon` tree the audit already parsed.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::ops::Range;

use full_moon::ast::{
    Ast, Call, Expression, FunctionArgs, FunctionBody, FunctionCall, Index, Prefix, Suffix, Var,
    VarExpression,
};
use full_moon::node::Node;
use full_moon::visitors::Visitor;
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::resolve::{args, dotted, ident, Resolver, Val};
use crate::{full_name, Check, Finding, Parsed, Severity, Side};

/// Globals Roblox deprecated for the `task` library.
const DEPRECATED_GLOBALS: [(&str, &str); 3] = [
    ("wait", "task.wait"),
    ("spawn", "task.spawn"),
    ("delay", "task.delay"),
];

/// DataStore and MemoryStore methods. `GetAsync` is `HttpService`'s too,
/// so it counts only on a receiver known to be a store.
const STORE_METHODS: [&str; 11] = [
    "GetAsync",
    "SetAsync",
    "UpdateAsync",
    "IncrementAsync",
    "RemoveAsync",
    "GetSortedAsync",
    "ListKeysAsync",
    "GetVersionAsync",
    "ReadAsync",
    "AddAsync",
    "GetRangeAsync",
];
/// The calls that hand out a store.
const STORE_GETTERS: [&str; 6] = [
    "GetDataStore",
    "GetOrderedDataStore",
    "GetGlobalDataStore",
    "GetSortedMap",
    "GetQueue",
    "GetHashMap",
];
/// Calls whose function argument runs protected.
const PROTECTORS: [&str; 5] = [
    "pcall",
    "xpcall",
    "Promise.new",
    "Promise.try",
    "Promise.defer",
];

/// The replacement for a deprecated method, where it isn't just the
/// capitalized name.
const REPLACEMENTS: [(&str, &str); 8] = [
    ("FindPartOnRay", "workspace:Raycast"),
    ("FindPartOnRayWithIgnoreList", "workspace:Raycast"),
    ("FindPartOnRayWithWhitelist", "workspace:Raycast"),
    ("FindPartsInRegion3", "workspace:GetPartBoundsInBox"),
    (
        "FindPartsInRegion3WithIgnoreList",
        "workspace:GetPartBoundsInBox",
    ),
    ("SetPrimaryPartCFrame", ":PivotTo"),
    ("GetPrimaryPartCFrame", ":GetPivot"),
    ("GetModelCFrame", ":GetPivot"),
];

/// `ModelStreamingMode.Atomic`: such a model streams in as a whole.
const ATOMIC: u32 = 1;

struct Place<'a> {
    dom: &'a WeakDom,
    db: &'a ReflectionDatabase,
    /// Method names some script in the place defines (`function X:play()`):
    /// a call to one isn't a call to Roblox's deprecated method of the
    /// same name.
    defined: HashSet<String>,
    always_deprecated: BTreeSet<&'a str>,
    streaming: bool,
}

pub(super) fn check(dom: &WeakDom, db: &ReflectionDatabase, scripts: &Parsed) -> Vec<Finding> {
    let defined = scripts
        .iter()
        .flat_map(|(_, _, ast)| {
            let mut methods = Methods(HashSet::new());
            methods.visit_ast(ast);
            methods.0
        })
        .collect();
    let streaming = dom.root_refs().iter().any(|r| {
        dom.get(*r).is_some_and(|i| {
            i.class() == "Workspace"
                && i.properties().get("StreamingEnabled") == Some(&Variant::Bool(true))
        })
    });
    let place = Place {
        dom,
        db,
        defined,
        always_deprecated: db.always_deprecated_functions(),
        streaming,
    };
    scripts
        .iter()
        .flat_map(|(script, side, ast)| place.script(*script, *side, ast))
        .collect()
}

/// Method names declared with `:` or as `X.name = function`.
struct Methods(HashSet<String>);

impl Visitor for Methods {
    fn visit_function_declaration(&mut self, node: &full_moon::ast::FunctionDeclaration) {
        let name = node.name();
        let last = name
            .method_name()
            .or_else(|| name.names().iter().last())
            .map(ident);
        self.0.extend(last.map(str::to_string));
    }
}

impl Place<'_> {
    fn script(&self, script: Ref, side: Side, ast: &Ast) -> Vec<Finding> {
        let mut scan = Scan::default();
        scan.visit_ast(ast);
        let protected = scan.protected();
        let mut lint = Lint {
            place: self,
            script,
            side,
            resolver: Resolver {
                dom: self.dom,
                script,
                bindings: HashMap::new(),
            },
            scan,
            protected,
            findings: Vec::new(),
        };
        lint.visit_ast(ast);
        lint.findings
    }
}

/// The first pass: what the script declares, before any call is judged.
#[derive(Default)]
struct Scan {
    /// Names declared local anywhere (locals, parameters, loop variables,
    /// local functions): `wait` declared so is the script's own.
    locals: HashSet<String>,
    /// Plain-named functions' bodies, by name.
    functions: HashMap<String, Range<usize>>,
    /// Names and fields a store was assigned to.
    stores: HashSet<String>,
    /// Instance paths assigned to locals, in source order.
    bindings: Vec<(String, Expression)>,
    /// Function bodies passed straight to a protector.
    protected: Vec<Range<usize>>,
    /// Functions passed to a protector by name.
    protected_names: Vec<String>,
    /// Plain calls `name(...)`, by name and where.
    calls: Vec<(String, usize)>,
}

impl Scan {
    /// Every protected byte range: bodies passed to a protector and, to a
    /// fixed point, the bodies of named functions called from inside one.
    fn protected(&self) -> Vec<Range<usize>> {
        let mut names: HashSet<&str> = self.protected_names.iter().map(String::as_str).collect();
        loop {
            let mut ranges = self.protected.clone();
            ranges.extend(names.iter().filter_map(|n| self.functions.get(*n).cloned()));
            let before = names.len();
            for (name, at) in &self.calls {
                if ranges.iter().any(|r| r.contains(at)) {
                    names.insert(name);
                }
            }
            if names.len() == before {
                return ranges;
            }
        }
    }
}

fn span(node: &impl Node) -> Option<Range<usize>> {
    Some(node.start_position()?.bytes()..node.end_position()?.bytes())
}

fn line(node: &impl Node) -> usize {
    node.start_position().map_or(0, |p| p.line())
}

/// The call chain's suffixes up to the first call: `Promise.try` of
/// `Promise.try(f)`.
fn callee(call: &FunctionCall) -> (Option<String>, Option<&FunctionArgs>) {
    let suffixes: Vec<&Suffix> = call.suffixes().collect();
    match suffixes
        .iter()
        .position(|s| matches!(s, Suffix::Call(_)))
        .map(|i| (i, suffixes[i]))
    {
        Some((i, Suffix::Call(Call::AnonymousCall(a)))) => {
            (dotted(call.prefix(), &suffixes[..i]), Some(a))
        }
        _ => (None, None),
    }
}

/// Whether an expression hands out a store: it calls one of
/// [`STORE_GETTERS`].
fn makes_store(expr: &Expression) -> bool {
    let Expression::FunctionCall(call) = expr else {
        return false;
    };
    call.suffixes().any(|s| {
        matches!(s, Suffix::Call(Call::MethodCall(m)) if STORE_GETTERS.contains(&ident(m.name())))
    })
}

impl Visitor for Scan {
    fn visit_local_assignment(&mut self, node: &full_moon::ast::LocalAssignment) {
        let names: Vec<String> = node.names().iter().map(|n| ident(n).to_string()).collect();
        for (name, expr) in names.iter().zip(node.expressions().iter()) {
            if makes_store(expr) {
                self.stores.insert(name.clone());
            }
            self.bindings.push((name.clone(), expr.clone()));
        }
        self.locals.extend(names);
    }

    fn visit_assignment(&mut self, node: &full_moon::ast::Assignment) {
        for (var, expr) in node.variables().iter().zip(node.expressions().iter()) {
            if !makes_store(expr) {
                continue;
            }
            let name = match var {
                Var::Name(name) => Some(ident(name)),
                Var::Expression(var) => match var.suffixes().last() {
                    Some(Suffix::Index(Index::Dot { name, .. })) => Some(ident(name)),
                    _ => None,
                },
                _ => None,
            };
            self.stores.extend(name.map(str::to_string));
        }
    }

    fn visit_local_function(&mut self, node: &full_moon::ast::LocalFunction) {
        let name = ident(node.name()).to_string();
        self.functions
            .extend(span(node.body()).map(|r| (name.clone(), r)));
        self.locals.insert(name);
    }

    fn visit_function_declaration(&mut self, node: &full_moon::ast::FunctionDeclaration) {
        let name = node.name();
        if name.method_name().is_none() {
            let path: Vec<&str> = name.names().iter().map(ident).collect();
            self.functions
                .extend(span(node.body()).map(|r| (path.join("."), r)));
        }
    }

    fn visit_function_body(&mut self, node: &FunctionBody) {
        for parameter in node.parameters() {
            if let full_moon::ast::Parameter::Name(name) = parameter {
                self.locals.insert(ident(name).to_string());
            }
        }
    }

    fn visit_generic_for(&mut self, node: &full_moon::ast::GenericFor) {
        self.locals
            .extend(node.names().iter().map(|n| ident(n).to_string()));
    }

    fn visit_numeric_for(&mut self, node: &full_moon::ast::NumericFor) {
        self.locals.insert(ident(node.index_variable()).to_string());
    }

    fn visit_function_call(&mut self, node: &FunctionCall) {
        let (name, call_args) = callee(node);
        let Some(name) = name else { return };
        if let Some(at) = node.start_position() {
            self.calls.push((name.clone(), at.bytes()));
        }
        if !PROTECTORS.contains(&name.as_str()) {
            return;
        }
        for arg in call_args.map(args).unwrap_or_default() {
            match arg {
                Expression::Function(function) => self.protected.extend(span(function.body())),
                Expression::Var(Var::Name(name)) => self.protected_names.push(ident(name).into()),
                _ => {}
            }
        }
    }
}

/// The second pass: judges each call and index chain.
struct Lint<'a> {
    place: &'a Place<'a>,
    script: Ref,
    side: Side,
    resolver: Resolver<'a>,
    scan: Scan,
    protected: Vec<Range<usize>>,
    findings: Vec<Finding>,
}

impl Lint<'_> {
    fn push(&mut self, check: Check, severity: Severity, line: usize, message: String) {
        self.findings.push(Finding {
            check,
            severity,
            instance: self.script,
            path: full_name(self.place.dom, self.script),
            line: Some(line),
            message,
        });
    }

    /// Binds every local assigned before byte `at`, so a receiver resolves
    /// through the locals above it.
    fn bind_until(&mut self, at: usize) {
        while let Some((name, expr)) = self.scan.bindings.first() {
            if expr.start_position().is_some_and(|p| p.bytes() > at) {
                break;
            }
            let val = self.resolver.expr(expr);
            self.resolver.bindings.insert(name.clone(), val);
            self.scan.bindings.remove(0);
        }
    }

    /// The class a resolved receiver has, when it is known.
    fn class_of(&self, val: &Val) -> Option<String> {
        match val {
            Val::Game => Some("DataModel".into()),
            Val::Inst(r) => self.place.dom.get(*r).map(|i| i.class().to_string()),
            Val::Service(class) => Some(class.clone()),
            _ => None,
        }
    }

    fn chain(&mut self, prefix: &Prefix, suffixes: &[&Suffix]) {
        if let Prefix::Name(base) = prefix {
            if matches!(ident(base), "game" | "Game")
                && !self.scan.locals.contains(ident(base))
                && matches!(suffixes.first(), Some(Suffix::Index(Index::Dot { name, .. })) if matches!(ident(name), "Workspace" | "workspace"))
            {
                self.push(
                    Check::DeprecatedApi,
                    Severity::Low,
                    line(base),
                    "`game.Workspace` finds the Workspace by name: use `workspace`.".into(),
                );
            }
        }
        if self.side == Side::Client && self.place.streaming {
            self.streamed(prefix, suffixes);
        }
        for (i, suffix) in suffixes.iter().enumerate() {
            let Suffix::Call(Call::MethodCall(method)) = suffix else {
                continue;
            };
            let name = ident(method.name());
            let at = method.name().start_position().map_or(0, |p| p.bytes());
            let line = line(method.name());
            if matches!(name, "connect" | "disconnect") {
                let fixed = format!("{}{}", name[..1].to_uppercase(), &name[1..]);
                self.push(
                    Check::DeprecatedApi,
                    Severity::Low,
                    line,
                    format!("`:{name}` is deprecated: use `:{fixed}`."),
                );
                continue;
            }
            if STORE_METHODS.contains(&name) {
                self.store_call(prefix, &suffixes[..i], name, at, line);
            }
            self.deprecated_method(prefix, &suffixes[..i], name, line);
        }
    }

    fn store_call(
        &mut self,
        prefix: &Prefix,
        receiver: &[&Suffix],
        name: &str,
        at: usize,
        line: usize,
    ) {
        let last = match receiver.last() {
            Some(Suffix::Index(Index::Dot { name, .. })) => Some(ident(name)),
            None => match prefix {
                Prefix::Name(n) => Some(ident(n)),
                _ => None,
            },
            _ => None,
        };
        let known = last.is_some_and(|n| self.scan.stores.contains(n))
            || receiver.iter().any(|s| {
                matches!(s, Suffix::Call(Call::MethodCall(m)) if STORE_GETTERS.contains(&ident(m.name())))
            });
        let store = known || (name != "GetAsync" && !self.place.defined.contains(name));
        if store && !self.protected.iter().any(|r| r.contains(&at)) {
            self.push(
                Check::DataStoreWithoutPcall,
                Severity::Medium,
                line,
                format!("`:{name}` isn't inside `pcall`: when Roblox's data stores throttle or fail, it throws and stops this script. Wrap it in `pcall` and handle the failure."),
            );
        }
    }

    fn deprecated_method(
        &mut self,
        prefix: &Prefix,
        receiver: &[&Suffix],
        name: &str,
        line: usize,
    ) {
        if receiver.is_empty() && matches!(prefix, Prefix::Name(n) if ident(n) == "self") {
            return;
        }
        let db = self.place.db;
        let val = self.resolver.chain(prefix, receiver);
        let deprecated = match self.class_of(&val) {
            Some(class) => db.function_deprecated(&class, name) == Some(true),
            None => {
                self.place.always_deprecated.contains(name) && !self.place.defined.contains(name)
            }
        };
        if !deprecated {
            return;
        }
        let replacement = REPLACEMENTS
            .iter()
            .find(|(old, _)| *old == name)
            .map(|(_, new)| new.to_string())
            .or_else(|| {
                let capital = format!("{}{}", name[..1].to_uppercase(), &name[1..]);
                (capital != name && !self.place.always_deprecated.contains(capital.as_str()))
                    .then(|| format!(":{capital}"))
            });
        let message = match replacement {
            Some(new) => format!("`:{name}` is deprecated: use `{new}`."),
            None => format!("`:{name}` is deprecated."),
        };
        self.push(Check::DeprecatedApi, Severity::Low, line, message);
    }

    /// A `.` index from the Workspace down onto a part that may not have
    /// streamed in yet: the first one in the chain, before any call.
    fn streamed(&mut self, prefix: &Prefix, suffixes: &[&Suffix]) {
        let dom = self.place.dom;
        let workspace = |val: &Val| matches!(val, Val::Inst(r) if dom.get(*r).is_some_and(|i| i.class() == "Workspace"));
        if !workspace(&self.resolver.chain(prefix, &[])) {
            return;
        }
        for (i, suffix) in suffixes.iter().enumerate() {
            let Suffix::Index(Index::Dot { name, .. }) = suffix else {
                return;
            };
            let Val::Inst(r) = self.resolver.chain(prefix, &suffixes[..=i]) else {
                return;
            };
            let Some(instance) = dom.get(r) else { return };
            let streamed = self.place.db.is_subclass_of(instance.class(), "BasePart")
                || (instance.class() == "Model"
                    && instance.properties().get("ModelStreamingMode")
                        == Some(&Variant::Enum(ATOMIC)));
            if streamed {
                let name = ident(name);
                self.push(
                    Check::StreamingIndex,
                    Severity::Low,
                    line(*suffix),
                    format!("With StreamingEnabled, `{name}` may not have streamed in when this runs: use `:WaitForChild(\"{name}\")`."),
                );
                return;
            }
        }
    }
}

impl Visitor for Lint<'_> {
    fn visit_function_call(&mut self, node: &FunctionCall) {
        if let Some(at) = node.start_position() {
            self.bind_until(at.bytes());
        }
        let suffixes: Vec<&Suffix> = node.suffixes().collect();
        if let (Prefix::Name(base), Some(Suffix::Call(Call::AnonymousCall(_)))) =
            (node.prefix(), suffixes.first())
        {
            let name = ident(base);
            if let Some((_, new)) = DEPRECATED_GLOBALS.iter().find(|(old, _)| *old == name) {
                if !self.scan.locals.contains(name) {
                    self.push(
                        Check::DeprecatedApi,
                        Severity::Low,
                        line(base),
                        format!("`{name}` is deprecated: use `{new}`, which doesn't throttle."),
                    );
                }
            }
        }
        if let (Some(name), Some(call_args)) = callee(node) {
            if name == "Instance.new" && args(call_args).len() == 2 {
                self.push(
                    Check::DeprecatedApi,
                    Severity::Low,
                    line(node),
                    "`Instance.new(class, parent)` is deprecated: set `Parent` last, after the other properties.".into(),
                );
            }
        }
        self.chain(node.prefix(), &suffixes);
    }

    fn visit_var_expression(&mut self, node: &VarExpression) {
        if let Some(at) = node.start_position() {
            self.bind_until(at.bytes());
        }
        let suffixes: Vec<&Suffix> = node.suffixes().collect();
        self.chain(node.prefix(), &suffixes);
    }
}
