//! One walk over a script: bindings in source order, call-site checks, and
//! the taint pass inside remote handlers.

use std::collections::{HashMap, HashSet};

use full_moon::ast::{
    Ast, Block, Call, Expression, Field, FunctionArgs, FunctionBody, If, Index, LastStmt,
    Parameter, Prefix, Stmt, Suffix, UnOp, Var, VarExpression,
};
use full_moon::node::Node;
use full_moon::tokenizer::TokenType;
use full_moon::visitors::Visitor;
use rbx_dom::{Ref, WeakDom};

use crate::resolve::{args, dotted, first_string, ident, string, Resolver, Val};
use crate::{full_name, Check, Finding, LorenService, RequireSite, Severity, Side, Usage};
use rbx_loren::graph::RouteKind;

/// Names a script assigns to `.Name` from a string literal: instances it
/// creates at run time, which a static path can't see.
pub(crate) fn runtime_names(ast: &Ast) -> HashSet<String> {
    struct Names(HashSet<String>);
    impl Visitor for Names {
        fn visit_assignment(&mut self, node: &full_moon::ast::Assignment) {
            for (var, expr) in node.variables().iter().zip(node.expressions().iter()) {
                if let (Var::Expression(var), Some(name)) = (var, string(expr)) {
                    if let Some(Suffix::Index(Index::Dot { name: field, .. })) =
                        var.suffixes().last()
                    {
                        if ident(field) == "Name" {
                            self.0.insert(name.to_string());
                        }
                    }
                }
            }
        }
    }
    let mut names = Names(HashSet::new());
    names.visit_ast(ast);
    names.0
}

/// Every named function in the script, by the name a handler would be
/// passed as or called by (`onBuy`, `Shop.onBuy`, `Shop:give`).
fn functions(ast: &Ast) -> HashMap<String, FunctionBody> {
    struct Functions(HashMap<String, FunctionBody>);
    impl Visitor for Functions {
        fn visit_local_function(&mut self, node: &full_moon::ast::LocalFunction) {
            self.0
                .insert(ident(node.name()).to_string(), node.body().clone());
        }
        fn visit_function_declaration(&mut self, node: &full_moon::ast::FunctionDeclaration) {
            let name = node.name();
            let path: Vec<&str> = name.names().iter().map(ident).collect();
            let key = match name.method_name() {
                Some(method) => format!("{}:{}", path.join("."), ident(method)),
                None => path.join("."),
            };
            self.0.insert(key, node.body().clone());
        }
    }
    let mut functions = Functions(HashMap::new());
    functions.visit_ast(ast);
    functions.0
}

pub(crate) fn script(
    dom: &WeakDom,
    script: Ref,
    side: Side,
    ast: &Ast,
    runtime_names: &HashSet<String>,
    loren: Option<&LorenService>,
    usage: &mut Usage,
) -> Vec<Finding> {
    let mut walker = Walker {
        resolver: Resolver {
            dom,
            script,
            bindings: HashMap::new(),
        },
        side,
        path: full_name(dom, script),
        functions: functions(ast),
        runtime_names,
        loren,
        usage,
        taint: None,
        calls: Vec::new(),
        findings: Vec::new(),
    };
    walker.block(ast.nodes());
    if side == Side::Client {
        walker.dotted_server_containers(ast);
    }
    walker.findings
}

/// Inside a remote handler: which names hold client-supplied data.
#[derive(Clone)]
struct Taint {
    remote: String,
    /// The handler's first parameter, which is always the sending Player.
    player: Option<String>,
    tainted: HashSet<String>,
    /// Names an enclosing condition has checked.
    guarded: HashSet<String>,
    /// Numbers (`tonumber(x)`, or `typeof(x) == "number"` checked): only a
    /// lower and an upper bound validate them, not a nil check.
    numeric: HashSet<String>,
    /// Names compared against a lower and against an upper bound.
    lower: HashSet<String>,
    upper: HashSet<String>,
    /// A Loren `Spec` or `Middleware` entry that runs before the handler.
    gate: Option<String>,
}

impl Taint {
    fn new(remote: String, player: Option<String>, tainted: HashSet<String>) -> Self {
        Taint {
            remote,
            player,
            tainted,
            guarded: HashSet::new(),
            numeric: HashSet::new(),
            lower: HashSet::new(),
            upper: HashSet::new(),
            gate: None,
        }
    }
}

/// How deep taint follows calls into the script's own functions.
const MAX_CALL_DEPTH: usize = 4;

struct Walker<'a> {
    resolver: Resolver<'a>,
    side: Side,
    path: String,
    functions: HashMap<String, FunctionBody>,
    runtime_names: &'a HashSet<String>,
    loren: Option<&'a LorenService>,
    usage: &'a mut Usage,
    taint: Option<Taint>,
    /// The functions taint is being followed into, innermost last.
    calls: Vec<String>,
    findings: Vec<Finding>,
}

/// Results that are a checked value however the input was shaped.
/// `Instance.new` is here because the instance is the server's own, even
/// when its class isn't: the call itself is the sink.
/// `tonumber` isn't one: it makes a number, but not a bounded one.
const SANITIZERS: [&str; 4] = ["math.clamp", "typeof", "type", "Instance.new"];

/// Writing a client-supplied value to these is the classic exploit.
const SENSITIVE: [&str; 13] = [
    "Value",
    "Health",
    "MaxHealth",
    "WalkSpeed",
    "JumpPower",
    "JumpHeight",
    "CFrame",
    "Position",
    "Size",
    "Orientation",
    "Parent",
    "AssemblyLinearVelocity",
    "Velocity",
];

const CURRENCY: [&str; 9] = [
    "leaderstats",
    "coin",
    "cash",
    "gold",
    "gem",
    "money",
    "point",
    "credit",
    "currency",
];

/// What a handler's first parameter is called when its author knows it's
/// the sending Player; any other name says they expected the client's data.
const PLAYER_NAMES: [&str; 12] = [
    "pl",
    "plr",
    "client",
    "sender",
    "user",
    "who",
    "owner",
    "caller",
    "requester",
    "from",
    "source",
    "self",
];

const DATASTORE_WRITES: [&str; 4] = ["SetAsync", "UpdateAsync", "IncrementAsync", "RemoveAsync"];

impl Walker<'_> {
    fn finding(&mut self, check: Check, severity: Severity, line: usize, message: String) {
        self.findings.push(Finding {
            check,
            severity,
            instance: self.resolver.script,
            path: self.path.clone(),
            line: Some(line),
            message,
        });
    }

    fn block(&mut self, block: &Block) {
        let saved = self.taint.clone();
        for stmt in block.stmts() {
            self.stmt(stmt);
        }
        if let Some(LastStmt::Return(ret)) = block.last_stmt() {
            for expr in ret.returns().iter() {
                self.expr(expr);
            }
        }
        self.taint = saved;
    }

    fn stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::LocalAssignment(local) => {
                let exprs: Vec<&Expression> = local.expressions().iter().collect();
                for expr in &exprs {
                    self.expr(expr);
                }
                for (i, name) in local.names().iter().enumerate() {
                    self.bind(ident(name), exprs.get(i).copied());
                    if let Some(Expression::TableConstructor(table)) = exprs.get(i) {
                        self.loren_table(ident(name), table);
                    }
                }
            }
            Stmt::Assignment(assign) => {
                let exprs: Vec<&Expression> = assign.expressions().iter().collect();
                for (i, var) in assign.variables().iter().enumerate() {
                    let expr = exprs.get(i).copied();
                    match var {
                        Var::Name(name) => {
                            if let Some(expr) = expr {
                                self.expr(expr);
                            }
                            self.bind(ident(name), expr);
                        }
                        Var::Expression(var) => {
                            let suffixes: Vec<&Suffix> = var.suffixes().collect();
                            self.chain(var.prefix(), &suffixes);
                            let Some(expr) = expr else { continue };
                            if last_field(&suffixes) == Some("OnServerInvoke") {
                                let remote = self
                                    .resolver
                                    .chain(var.prefix(), &suffixes[..suffixes.len() - 1]);
                                self.listener(&remote, line(var));
                                self.handler_arg(&remote, expr);
                            } else if let (Some(route), Expression::Function(f)) = (
                                dotted(var.prefix(), &suffixes)
                                    .and_then(|path| self.loren_route(&path, false)),
                                expr,
                            ) {
                                self.handler(route.0, f.body(), route.1, route.2);
                            } else {
                                self.expr(expr);
                                self.assigned(var, expr);
                            }
                        }
                        _ => {}
                    }
                }
                for expr in exprs.iter().skip(assign.variables().len()) {
                    self.expr(expr);
                }
            }
            Stmt::CompoundAssignment(compound) => {
                self.expr(compound.rhs());
                match compound.lhs() {
                    Var::Expression(var) => self.assigned(var, compound.rhs()),
                    Var::Name(name) if self.tainted(compound.rhs()) => {
                        if let Some(taint) = &mut self.taint {
                            taint.tainted.insert(ident(name).to_string());
                        }
                    }
                    _ => {}
                }
            }
            Stmt::FunctionCall(call) => {
                self.chain(call.prefix(), &call.suffixes().collect::<Vec<_>>());
            }
            Stmt::Do(block) => self.block(block.block()),
            Stmt::While(w) => {
                self.expr(w.condition());
                self.block(w.block());
            }
            Stmt::Repeat(r) => {
                self.block(r.block());
                self.expr(r.until());
            }
            Stmt::If(branch) => self.if_stmt(branch),
            Stmt::NumericFor(f) => {
                self.expr(f.start());
                self.expr(f.end());
                if let Some(step) = f.step() {
                    self.expr(step);
                }
                self.block(f.block());
            }
            Stmt::GenericFor(f) => {
                let tainted = f.expressions().iter().any(|e| self.tainted(e));
                for expr in f.expressions().iter() {
                    self.expr(expr);
                }
                let saved = self.taint.clone();
                if let Some(taint) = &mut self.taint {
                    for name in f.names().iter() {
                        let name = ident(name).to_string();
                        match tainted {
                            true => taint.tainted.insert(name),
                            false => taint.tainted.remove(&name),
                        };
                    }
                }
                self.block(f.block());
                self.taint = saved;
            }
            Stmt::LocalFunction(f) => self.function(f.body()),
            Stmt::FunctionDeclaration(f) => {
                let name = f.name();
                let mut path: Vec<&str> = name.names().iter().map(ident).collect();
                let colon = name.method_name().map(ident);
                path.extend(colon);
                match self.loren_route(&path.join("."), colon.is_some()) {
                    Some((remote, player, gate)) => self.handler(remote, f.body(), player, gate),
                    None => self.function(f.body()),
                }
            }
            _ => {}
        }
    }

    fn bind(&mut self, name: &str, expr: Option<&Expression>) {
        let val = expr.map_or(Val::Unknown, |e| self.resolver.expr(e));
        let tainted = expr.is_some_and(|e| self.tainted(e));
        self.resolver.bindings.insert(name.to_string(), val);
        let numeric = expr.is_some_and(is_tonumber);
        if let Some(taint) = &mut self.taint {
            taint.guarded.remove(name);
            taint.lower.remove(name);
            taint.upper.remove(name);
            match tainted {
                true => taint.tainted.insert(name.to_string()),
                false => taint.tainted.remove(name),
            };
            match tainted && numeric {
                true => taint.numeric.insert(name.to_string()),
                false => taint.numeric.remove(name),
            };
        }
    }

    fn if_stmt(&mut self, branch: &If) {
        self.expr(branch.condition());
        let mut failed = Vec::new();
        self.guarded_block(&failed, Some(branch.condition()), branch.block());
        failed.push(branch.condition());
        for elseif in branch.else_if().into_iter().flatten() {
            self.expr(elseif.condition());
            self.guarded_block(&failed, Some(elseif.condition()), elseif.block());
            failed.push(elseif.condition());
        }
        if let Some(block) = branch.else_block() {
            self.guarded_block(&failed, None, block);
        }
        // `if not valid(x) then return end` guards the rest of the block.
        if branch.else_if().is_none() && branch.else_block().is_none() && exits(branch.block()) {
            self.guard(branch.condition(), true);
        }
    }

    /// A branch runs with the earlier conditions false and its own true.
    fn guarded_block(&mut self, failed: &[&Expression], holds: Option<&Expression>, block: &Block) {
        let saved = self.taint.clone();
        for condition in failed {
            self.guard(condition, true);
        }
        if let Some(condition) = holds {
            self.guard(condition, false);
        }
        self.block(block);
        self.taint = saved;
    }

    fn guard(&mut self, condition: &Expression, negated: bool) {
        if let Some(taint) = &mut self.taint {
            guard(taint, condition, negated);
        }
    }

    fn function(&mut self, body: &FunctionBody) {
        self.function_with(body, None);
    }

    /// Walks a function's body with its parameters shadowing outer names;
    /// a remote handler's walk starts from its own `taint` instead.
    fn function_with(&mut self, body: &FunctionBody, handler: Option<Taint>) {
        let saved_taint = self.taint.clone();
        let params = params(body);
        let saved: Vec<_> = params
            .iter()
            .map(|p| {
                (
                    p.clone(),
                    self.resolver.bindings.insert(p.clone(), Val::Unknown),
                )
            })
            .collect();
        match handler {
            Some(taint) => self.taint = Some(taint),
            None => {
                if let Some(taint) = &mut self.taint {
                    for p in &params {
                        taint.tainted.remove(p);
                    }
                }
            }
        }
        self.block(body.block());
        for (name, val) in saved {
            match val {
                Some(val) => self.resolver.bindings.insert(name, val),
                None => self.resolver.bindings.remove(&name),
            };
        }
        self.taint = saved_taint;
    }

    fn expr(&mut self, expr: &Expression) {
        match expr {
            Expression::BinaryOperator { lhs, binop, rhs } => {
                self.player_as_data(lhs, &binop.token().to_string(), rhs);
                self.expr(lhs);
                self.expr(rhs);
            }
            Expression::Parentheses { expression, .. }
            | Expression::UnaryOperator { expression, .. }
            | Expression::TypeAssertion { expression, .. } => self.expr(expression),
            Expression::Function(f) => self.function(f.body()),
            Expression::FunctionCall(call) => {
                self.chain(call.prefix(), &call.suffixes().collect::<Vec<_>>())
            }
            Expression::Var(Var::Expression(var)) => {
                self.chain(var.prefix(), &var.suffixes().collect::<Vec<_>>())
            }
            Expression::IfExpression(e) => {
                self.expr(e.condition());
                self.expr(e.if_expression());
                for elseif in e.else_if_expressions().into_iter().flatten() {
                    self.expr(elseif.condition());
                    self.expr(elseif.expression());
                }
                self.expr(e.else_expression());
            }
            Expression::InterpolatedString(s) => {
                for e in s.expressions() {
                    self.expr(e);
                }
            }
            Expression::TableConstructor(table) => {
                for field in table.fields().iter() {
                    match field {
                        Field::ExpressionKey { key, value, .. } => {
                            self.expr(key);
                            self.expr(value);
                        }
                        Field::NameKey { value, .. } => self.expr(value),
                        Field::NoKey(value) => self.expr(value),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn chain(&mut self, prefix: &Prefix, suffixes: &[&Suffix]) {
        if let Prefix::Expression(expr) = prefix {
            self.expr(expr);
        }
        for (i, suffix) in suffixes.iter().enumerate() {
            match suffix {
                Suffix::Index(Index::Brackets { expression, .. }) => self.expr(expression),
                Suffix::Call(call) => self.call(prefix, suffixes, i, call),
                _ => {}
            }
        }
    }

    fn call(&mut self, prefix: &Prefix, suffixes: &[&Suffix], i: usize, call: &Call) {
        let before = &suffixes[..i];
        match call {
            Call::MethodCall(method) => {
                let name = ident(method.name());
                let line = line(method.name());
                let arguments = args(method.args());
                if matches!(name, "Connect" | "Once" | "ConnectParallel" | "Wait")
                    && last_field(before) == Some("OnServerEvent")
                {
                    let remote = self.resolver.chain(prefix, &before[..before.len() - 1]);
                    self.listener(&remote, line);
                    if let Some(handler) = arguments.first().filter(|_| name != "Wait") {
                        self.handler_arg(&remote, handler);
                    }
                    return;
                }
                if let (Some((remote, gate)), Some(body), "Connect" | "Once") = (
                    self.loren_event(prefix, before),
                    arguments.first().and_then(|h| self.handler_body(h)),
                    name,
                ) {
                    self.handler(remote, &body, 0, gate);
                    return;
                }
                self.method(prefix, before, name, method.args(), line);
                if let Some(key) = self.method_key(prefix, before, name) {
                    self.follow(&key, &arguments);
                }
                for arg in arguments {
                    self.expr(arg);
                }
            }
            Call::AnonymousCall(call_args) => {
                let callee = dotted(prefix, before);
                let line = line(prefix);
                self.function_call(callee.as_deref(), call_args, line);
                if let Some(callee) = &callee {
                    self.follow(callee, &args(call_args));
                }
                for arg in args(call_args) {
                    self.expr(arg);
                }
            }
            _ => {}
        }
    }

    fn method(
        &mut self,
        prefix: &Prefix,
        before: &[&Suffix],
        name: &str,
        call_args: &FunctionArgs,
        line: usize,
    ) {
        let arguments = args(call_args);
        match name {
            "FireServer" | "InvokeServer" => {
                let remote = self.resolver.chain(prefix, before);
                match &remote {
                    Val::Inst(r) => {
                        self.usage.client_calls.insert(*r);
                    }
                    Val::Missing { path, first } if !self.runtime_names.contains(first) => {
                        self.finding(
                            Check::MissingRemote,
                            Severity::Medium,
                            line,
                            format!("{name} on `{path}`, which isn't in the place."),
                        );
                    }
                    _ => {}
                }
                if arguments
                    .first()
                    .is_some_and(|a| self.resolver.expr(a) == Val::LocalPlayer)
                {
                    self.finding(
                        Check::PlayerArgument,
                        Severity::Medium,
                        line,
                        format!("Passes LocalPlayer as {name}'s first argument: the server already receives the sending Player first, so every argument arrives shifted by one."),
                    );
                }
            }
            "FireClient" | "FireAllClients" | "InvokeClient" => {
                if let Val::Inst(r) = self.resolver.chain(prefix, before) {
                    self.usage.server_calls.insert(r);
                }
                if name == "InvokeClient" && self.side != Side::Client {
                    self.finding(
                        Check::InvokeClient,
                        Severity::Medium,
                        line,
                        "RemoteFunction:InvokeClient waits on a client: an exploiter can make it yield forever, error, or return anything. Use a RemoteEvent the client answers instead.".into(),
                    );
                }
            }
            "GetService" if self.side == Side::Client => {
                let service = first_string(call_args).unwrap_or_default();
                let severity = match service {
                    "DataStoreService" => Some(Severity::High),
                    "ServerStorage" | "ServerScriptService" => Some(Severity::Medium),
                    _ => None,
                };
                if let Some(severity) = severity {
                    self.finding(
                        Check::ServerApiOnClient,
                        severity,
                        line,
                        server_only_message(service),
                    );
                }
            }
            _ => {}
        }

        if self.taint.is_none() {
            return;
        }
        let receiver = self.chain_tainted(prefix, before);
        let tainted_arg = arguments.iter().any(|a| self.tainted(a));
        let sink = match name {
            "Destroy" | "Remove" | "ClearAllChildren" if receiver => Some(format!(
                "calls :{name}() on an instance the client passed in"
            )),
            n if DATASTORE_WRITES.contains(&n) && tainted_arg => {
                Some(format!("writes client-supplied data with :{name}()"))
            }
            "PivotTo" | "SetPrimaryPartCFrame" | "MoveTo" | "TranslateBy"
                if tainted_arg || receiver =>
            {
                Some(format!(
                    "moves an instance with :{name}() to a client-supplied place"
                ))
            }
            "TakeDamage" if tainted_arg || receiver => {
                Some("deals client-supplied damage with :TakeDamage()".into())
            }
            _ if tainted_arg && self.is_marketplace(prefix, before) => Some(format!(
                "passes client-supplied arguments to MarketplaceService:{name}()"
            )),
            _ => None,
        };
        if let Some(sink) = sink {
            self.sink(Severity::High, line, &sink);
        }
    }

    fn is_marketplace(&self, prefix: &Prefix, before: &[&Suffix]) -> bool {
        match self.resolver.chain(prefix, before) {
            Val::Service(class) => class == "MarketplaceService",
            Val::Inst(r) => self
                .resolver
                .dom
                .get(r)
                .is_some_and(|i| i.class() == "MarketplaceService"),
            _ => false,
        }
    }

    fn function_call(&mut self, callee: Option<&str>, call_args: &FunctionArgs, line: usize) {
        let arguments = args(call_args);
        match callee {
            Some("require") => {
                if let Some(target) = arguments.first() {
                    self.require(target, line);
                }
            }
            Some("assert") => {
                if let Some(condition) = arguments.first() {
                    self.guard(condition, false);
                }
            }
            Some("tonumber") => {
                if let Some(arg) = arguments.first() {
                    self.player_as_data(arg, "tonumber", arg);
                }
            }
            _ => {}
        }
        if self.taint.is_none() || !arguments.iter().any(|a| self.tainted(a)) {
            return;
        }
        let sink = match callee {
            Some("require") => Some((Severity::High, "requires a client-chosen module")),
            Some("loadstring") => {
                Some((Severity::High, "runs client-supplied code with loadstring"))
            }
            Some("Instance.new") => Some((
                Severity::Medium,
                "creates an instance of a client-chosen class or parent",
            )),
            _ => None,
        };
        if let Some((severity, sink)) = sink {
            self.sink(severity, line, sink);
        }
    }

    fn require(&mut self, target: &Expression, line: usize) {
        let val = self.resolver.expr(target);
        self.usage.requires.push(RequireSite {
            from: self.resolver.script,
            line,
            target: val.clone(),
            literal: string(target).map(str::to_owned),
            text: target.to_string().trim().to_owned(),
        });
        match val {
            Val::Inst(r) => {
                let class = self.resolver.dom.get(r).map_or("", |i| i.class());
                if class != "ModuleScript" {
                    let path = full_name(self.resolver.dom, r);
                    self.finding(
                        Check::BadRequire,
                        Severity::High,
                        line,
                        format!("require(`{path}`): that's a {class}, not a ModuleScript."),
                    );
                }
            }
            Val::Missing { path, first } if !self.runtime_names.contains(&first) => {
                self.finding(
                    Check::BadRequire,
                    Severity::High,
                    line,
                    format!("require(`{path}`): no such instance in the place."),
                );
            }
            _ => {}
        }
    }

    fn listener(&mut self, remote: &Val, line: usize) {
        match remote {
            Val::Inst(r) => {
                self.usage.listened.insert(*r);
            }
            Val::Missing { path, first } if !self.runtime_names.contains(first) => {
                self.finding(
                    Check::MissingRemote,
                    Severity::Medium,
                    line,
                    format!("Listens on `{path}`, which isn't in the place."),
                );
            }
            Val::Created => {}
            _ => self.usage.unresolved_listeners += 1,
        }
    }

    /// A handler passed by value: an anonymous function, or a named one.
    fn handler_arg(&mut self, remote: &Val, handler: &Expression) {
        let Some(body) = self.handler_body(handler) else {
            self.expr(handler);
            return;
        };
        let remote = match remote {
            Val::Inst(r) => full_name(self.resolver.dom, *r),
            Val::Missing { path, .. } => path.clone(),
            _ => "remote".into(),
        };
        self.handler(remote, &body, 0, None);
    }

    fn handler_body(&self, handler: &Expression) -> Option<FunctionBody> {
        match handler {
            Expression::Function(f) => Some(f.body().clone()),
            Expression::Var(Var::Name(name)) => self.functions.get(ident(name)).cloned(),
            Expression::Var(Var::Expression(var)) => {
                dotted(var.prefix(), &var.suffixes().collect::<Vec<_>>())
                    .and_then(|name| self.functions.get(&name).cloned())
            }
            _ => None,
        }
    }

    /// Walks a handler with the parameters after the Player (`params[player]`)
    /// tainted.
    fn handler(
        &mut self,
        remote: String,
        body: &FunctionBody,
        player: usize,
        gate: Option<String>,
    ) {
        let params = params(body);
        let first = player;
        let player = params.get(first).filter(|p| *p != "...").cloned();
        if let Some(p) = &player {
            let lower = p.to_lowercase();
            let playerish = p.len() == 1
                || p.starts_with('_')
                || PLAYER_NAMES.iter().any(|n| lower.contains(n));
            if !playerish {
                self.finding(
                    Check::PlayerArgument,
                    Severity::Medium,
                    line(body.parameters_parentheses()),
                    format!("The `{remote}` handler names its Player parameter `{p}`, but it always receives the sending Player; the client's arguments come after it."),
                );
            }
        }
        let mut tainted: HashSet<String> = params.iter().skip(first + 1).cloned().collect();
        if params.get(first).is_some_and(|p| p == "...") {
            tainted.insert("...".into());
        }
        let mut taint = Taint::new(remote, player, tainted);
        taint.gate = gate;
        self.function_with(body, Some(taint));
    }

    /// A call to one of the script's own functions with client data: the
    /// callee is walked with the parameters those arguments land in tainted.
    fn follow(&mut self, key: &str, arguments: &[&Expression]) {
        let Some(taint) = &self.taint else { return };
        if self.calls.len() >= MAX_CALL_DEPTH || self.calls.iter().any(|c| c == key) {
            return;
        }
        let Some(body) = self.functions.get(key).cloned() else {
            return;
        };
        let dirty: Vec<bool> = arguments.iter().map(|a| self.tainted(a)).collect();
        if !dirty.contains(&true) {
            return;
        }
        let mut tainted = HashSet::new();
        let mut player = None;
        for (i, p) in params(&body).into_iter().enumerate() {
            if p == "..." {
                if dirty.get(i..).is_some_and(|rest| rest.contains(&true)) {
                    tainted.insert(p);
                }
                break;
            }
            let Some(arg) = arguments.get(i) else { break };
            if dirty[i] {
                tainted.insert(p);
            } else if taint.player.as_ref().is_some_and(|name| is_name(arg, name)) {
                player = Some(p);
            }
        }
        let mut callee = Taint::new(taint.remote.clone(), player, tainted);
        callee.gate = taint.gate.clone();
        self.calls.push(key.to_string());
        self.function_with(&body, Some(callee));
        self.calls.pop();
    }

    /// The `functions` key a method call lands on: `Shop:give`, or for
    /// `self:give` the one method of that name the script declares.
    fn method_key(&self, prefix: &Prefix, before: &[&Suffix], name: &str) -> Option<String> {
        self.taint.as_ref()?;
        let key = format!("{}:{name}", dotted(prefix, before)?);
        if self.functions.contains_key(&key) {
            return Some(key);
        }
        let suffix = format!(":{name}");
        let mut matches = self.functions.keys().filter(|k| k.ends_with(&suffix));
        match (matches.next(), matches.next()) {
            (Some(only), None) if key.starts_with("self:") => Some(only.clone()),
            _ => None,
        }
    }

    /// The Loren route a function at `path` (`Shop.Client.Buy`,
    /// `Shop.Client:Buy`, `Shop.Middleware.Buy`) handles: its name, which
    /// parameter is the Player, and the gate that runs before it.
    fn loren_route(&self, path: &str, colon: bool) -> Option<(String, usize, Option<String>)> {
        let service = self.loren?;
        let rest = path.strip_prefix(service.name.as_str())?;
        let (table, name) = rest
            .strip_prefix(".Client")
            .map(|n| ("Client", n))
            .or_else(|| rest.strip_prefix(".Middleware").map(|n| ("Middleware", n)))?;
        let name = name.strip_prefix(['.', ':'])?;
        let route = service.routes.iter().find(|r| r.name == name)?;
        match table {
            "Client" if route.kind == RouteKind::Method => Some((
                format!("{}.Client.{name}", service.name),
                usize::from(!colon),
                gate(service, route),
            )),
            "Middleware" if route.middleware => {
                Some((format!("{}.Middleware.{name}", service.name), 0, None))
            }
            _ => None,
        }
    }

    /// `self.ClientEvents.X` / `Shop.ClientEvents.X` in a Loren Service: the
    /// route's name and its gate.
    fn loren_event(&self, prefix: &Prefix, before: &[&Suffix]) -> Option<(String, Option<String>)> {
        let service = self.loren?;
        let path = dotted(prefix, before)?;
        let (owner, name) = path.split_once(".ClientEvents.")?;
        if owner != "self" && owner != service.name {
            return None;
        }
        let route = service
            .routes
            .iter()
            .find(|r| r.name == name && r.kind == RouteKind::ClientEvent)?;
        Some((
            format!("{}.ClientEvents.{name}", service.name),
            gate(service, route),
        ))
    }

    /// `Middleware = { Buy = function(player, ...) ... end }` in a Loren
    /// Service's own table.
    fn loren_table(&mut self, local: &str, table: &full_moon::ast::TableConstructor) {
        if self.loren.is_none_or(|s| s.name != local) {
            return;
        }
        for field in table.fields().iter() {
            let Field::NameKey { key, value, .. } = field else {
                continue;
            };
            let Expression::TableConstructor(middleware) = &**value else {
                continue;
            };
            if ident(key) != "Middleware" {
                continue;
            }
            for entry in middleware.fields().iter() {
                let Field::NameKey { key, value, .. } = entry else {
                    continue;
                };
                if let Expression::Function(f) = &**value {
                    let path = format!("{local}.Middleware.{}", ident(key));
                    if let Some((remote, player, gate)) = self.loren_route(&path, false) {
                        self.handler(remote, f.body(), player, gate);
                    }
                }
            }
        }
    }

    fn sink(&mut self, severity: Severity, line: usize, what: &str) {
        let Some(taint) = &self.taint else { return };
        let mut message = format!(
            "The `{}` handler {what} without validating it first.",
            taint.remote
        );
        let severity = match &taint.gate {
            Some(gate) => {
                message.push_str(&format!(
                    " `{gate}` runs first: make sure it bounds this value."
                ));
                severity.lower()
            }
            None => severity,
        };
        self.finding(Check::UnvalidatedArgument, severity, line, message);
    }

    /// `x.Prop = value`: a sensitive property set from client data, or any
    /// property set on an instance the client chose.
    fn assigned(&mut self, var: &VarExpression, value: &Expression) {
        let Some(taint) = &self.taint else { return };
        let suffixes: Vec<&Suffix> = var.suffixes().collect();
        let Some(prop) = last_field(&suffixes) else {
            return;
        };
        let line = line(var);
        if let Some(player) = &taint.player {
            if prop == "Value" && is_name(value, player) {
                let message = format!("Stores the Player `{player}` in a Value: the first handler parameter is the sender, not the client's data.");
                self.finding(Check::PlayerArgument, Severity::Medium, line, message);
            }
        }
        let text = var.to_string().trim().to_string();
        let receiver = self.chain_tainted(var.prefix(), &suffixes[..suffixes.len() - 1]);
        // A lowercase field is a Lua table's (`args.n` on a `table.pack`), not
        // an Instance property.
        let property = prop.starts_with(|c: char| c.is_ascii_uppercase());
        if receiver && property {
            let severity = match SENSITIVE.contains(&prop) {
                true => Severity::High,
                false => Severity::Medium,
            };
            self.sink(
                severity,
                line,
                &format!("sets `{text}` on an instance the client passed in"),
            );
        } else if SENSITIVE.contains(&prop) && self.tainted(value) {
            let lower = text.to_lowercase();
            let severity = if prop != "Value" || CURRENCY.iter().any(|w| lower.contains(w)) {
                Severity::High
            } else {
                Severity::Medium
            };
            self.sink(
                severity,
                line,
                &format!("sets `{text}` from a client-supplied value"),
            );
        }
    }

    /// The handler's Player parameter used as a number or a string.
    fn player_as_data(&mut self, lhs: &Expression, op: &str, rhs: &Expression) {
        let Some(player) = self.taint.as_ref().and_then(|t| t.player.clone()) else {
            return;
        };
        let op = op.trim();
        let literal = |e: &Expression| matches!(e, Expression::Number(_) | Expression::String(_));
        let typeof_player = |e: &Expression| {
            match e {
            Expression::FunctionCall(call) => {
                matches!(call.prefix(), Prefix::Name(n) if matches!(ident(n), "typeof" | "type"))
                    && call.suffixes().any(|s| matches!(s, Suffix::Call(Call::AnonymousCall(a)) if args(a).first().is_some_and(|a| is_name(a, &player))))
            }
            _ => false,
        }
        };
        let confused = match op {
            "tonumber" => is_name(lhs, &player),
            "+" | "-" | "*" | "/" | "//" | "%" | "^" => {
                is_name(lhs, &player) || is_name(rhs, &player)
            }
            "==" | "~=" | "<" | ">" | "<=" | ">=" => {
                (is_name(lhs, &player) && literal(rhs))
                    || (is_name(rhs, &player) && literal(lhs))
                    || (typeof_player(lhs) && string(rhs).is_some_and(|s| s != "Instance"))
                    || (typeof_player(rhs) && string(lhs).is_some_and(|s| s != "Instance"))
            }
            _ => false,
        };
        if confused {
            self.finding(
                Check::PlayerArgument,
                Severity::Medium,
                line(lhs),
                format!("Treats `{player}` as data, but the first handler parameter is always the sending Player."),
            );
        }
    }

    fn name_tainted(&self, name: &str) -> bool {
        self.taint.as_ref().is_some_and(|t| {
            t.tainted.contains(name)
                && !t.guarded.contains(name)
                && !(t.lower.contains(name) && t.upper.contains(name))
        })
    }

    fn tainted(&self, expr: &Expression) -> bool {
        if self.taint.is_none() {
            return false;
        }
        match expr {
            Expression::Var(Var::Name(name)) => self.name_tainted(ident(name)),
            Expression::Var(Var::Expression(var)) => {
                self.chain_tainted(var.prefix(), &var.suffixes().collect::<Vec<_>>())
            }
            Expression::FunctionCall(call) => {
                self.chain_tainted(call.prefix(), &call.suffixes().collect::<Vec<_>>())
            }
            Expression::BinaryOperator { lhs, binop, rhs } => {
                let op = binop.token().to_string();
                !matches!(op.trim(), "==" | "~=" | "<" | ">" | "<=" | ">=")
                    && (self.tainted(lhs) || self.tainted(rhs))
            }
            Expression::UnaryOperator {
                unop: UnOp::Not(_), ..
            } => false,
            Expression::UnaryOperator { expression, .. }
            | Expression::Parentheses { expression, .. }
            | Expression::TypeAssertion { expression, .. } => self.tainted(expression),
            Expression::Symbol(token) => {
                token.to_string().trim() == "..." && self.name_tainted("...")
            }
            Expression::IfExpression(_)
            | Expression::InterpolatedString(_)
            | Expression::TableConstructor(_) => names(expr).iter().any(|n| self.name_tainted(n)),
            _ => false,
        }
    }

    /// Whether a chain's value is client data: a field of a tainted value,
    /// or a call fed one. Indexing a server table with client data
    /// (`Items[id]`) gives server data back.
    fn chain_tainted(&self, prefix: &Prefix, suffixes: &[&Suffix]) -> bool {
        let mut tainted = match prefix {
            Prefix::Name(name) => self.name_tainted(ident(name)),
            Prefix::Expression(expr) => self.tainted(expr),
            _ => false,
        };
        for (i, suffix) in suffixes.iter().enumerate() {
            tainted = match suffix {
                Suffix::Call(Call::AnonymousCall(call_args)) => {
                    let callee = dotted(prefix, &suffixes[..i]);
                    !callee.is_some_and(|c| SANITIZERS.contains(&c.as_str()))
                        && (tainted || args(call_args).iter().any(|a| self.tainted(a)))
                }
                Suffix::Call(Call::MethodCall(method)) => {
                    tainted || args(method.args()).iter().any(|a| self.tainted(a))
                }
                _ => tainted,
            };
        }
        tainted
    }

    /// `game.ServerStorage` and friends in a client script.
    fn dotted_server_containers(&mut self, ast: &Ast) {
        let mut previous_dot = false;
        let mut hits = Vec::new();
        for token in ast.nodes().tokens() {
            if let TokenType::Identifier { identifier } = token.token_type() {
                if previous_dot
                    && matches!(identifier.as_str(), "ServerStorage" | "ServerScriptService")
                {
                    hits.push((line(token), identifier.to_string()));
                }
            }
            previous_dot = token.to_string().trim() == ".";
        }
        for (line, service) in hits {
            self.finding(
                Check::ServerApiOnClient,
                Severity::Medium,
                line,
                server_only_message(&service),
            );
        }
    }
}

fn gate(service: &LorenService, route: &rbx_loren::graph::Route) -> Option<String> {
    let table = match (route.spec, route.middleware) {
        (true, _) => "Spec",
        (false, true) => "Middleware",
        _ => return None,
    };
    Some(format!("{}.{table}.{}", service.name, route.name))
}

/// What a condition (or, `negated`, its failing) says about the names in
/// it. A name compared with `<`/`>` gets one bound; anything else that
/// reads a name (a type check, a lookup, a call, a nil check) validates
/// it, except a number, which needs both bounds.
fn guard(taint: &mut Taint, condition: &Expression, negated: bool) {
    match condition {
        Expression::Parentheses { expression, .. } => guard(taint, expression, negated),
        Expression::UnaryOperator {
            unop: UnOp::Not(_),
            expression,
        } => guard(taint, expression, !negated),
        Expression::BinaryOperator { lhs, binop, rhs } => {
            let op = binop.token().to_string();
            match op.trim() {
                "and" | "or" => {
                    guard(taint, lhs, negated);
                    guard(taint, rhs, negated);
                }
                op @ ("<" | "<=" | ">" | ">=") => {
                    // `x < y`: an upper bound on x, a lower one on y.
                    let upper = matches!(op, "<" | "<=") != negated;
                    bound(taint, lhs, upper);
                    bound(taint, rhs, !upper);
                }
                "==" | "~=" => match number_check(lhs, rhs).or_else(|| number_check(rhs, lhs)) {
                    Some(name) => {
                        taint.numeric.insert(name);
                    }
                    None => validate(taint, condition),
                },
                _ => validate(taint, condition),
            }
        }
        _ => validate(taint, condition),
    }
}

fn bound(taint: &mut Taint, side: &Expression, upper: bool) {
    match side {
        Expression::Var(Var::Name(name)) => {
            let set = if upper {
                &mut taint.upper
            } else {
                &mut taint.lower
            };
            set.insert(ident(name).to_string());
        }
        _ => validate(taint, side),
    }
}

fn validate(taint: &mut Taint, expr: &Expression) {
    for name in names(expr) {
        if !taint.numeric.contains(&name) {
            taint.guarded.insert(name);
        }
    }
}

/// `typeof(x) == "number"`'s `x`.
fn number_check(call: &Expression, literal: &Expression) -> Option<String> {
    if string(literal) != Some("number") {
        return None;
    }
    let Expression::FunctionCall(call) = call else {
        return None;
    };
    if !matches!(call.prefix(), Prefix::Name(n) if matches!(ident(n), "typeof" | "type")) {
        return None;
    }
    call.suffixes().find_map(|s| match s {
        Suffix::Call(Call::AnonymousCall(a)) => match args(a).first() {
            Some(Expression::Var(Var::Name(n))) => Some(ident(n).to_string()),
            _ => None,
        },
        _ => None,
    })
}

/// `tonumber(x)`, or `tonumber(x) or default`.
fn is_tonumber(expr: &Expression) -> bool {
    match expr {
        Expression::FunctionCall(call) => {
            matches!(call.prefix(), Prefix::Name(n) if ident(n) == "tonumber")
        }
        Expression::BinaryOperator { lhs, binop, .. } => {
            binop.token().to_string().trim() == "or" && is_tonumber(lhs)
        }
        Expression::Parentheses { expression, .. } => is_tonumber(expression),
        _ => false,
    }
}

fn server_only_message(service: &str) -> String {
    match service {
        "DataStoreService" => "Uses DataStoreService from client code: data stores are server-only and error on the client.".into(),
        _ => format!("Reaches into {service} from client code: it never replicates to clients, so the client sees it empty."),
    }
}

fn line(node: &impl Node) -> usize {
    node.start_position().map_or(0, |p| p.line())
}

fn params(body: &FunctionBody) -> Vec<String> {
    body.parameters()
        .iter()
        .map(|p| match p {
            Parameter::Ellipsis(_) => "...".to_string(),
            Parameter::Name(name) => ident(name).to_string(),
            _ => String::new(),
        })
        .collect()
}

fn last_field<'a>(suffixes: &[&'a Suffix]) -> Option<&'a str> {
    match suffixes.last()? {
        Suffix::Index(Index::Dot { name, .. }) => Some(ident(name)),
        _ => None,
    }
}

fn is_name(expr: &Expression, name: &str) -> bool {
    matches!(expr, Expression::Var(Var::Name(n)) if ident(n) == name)
}

/// The variable names an expression reads, identifiers not after `.` or `:`.
fn names(expr: &Expression) -> HashSet<String> {
    let mut out = HashSet::new();
    let mut after_access = false;
    for token in expr.tokens() {
        let text = token.to_string();
        let text = text.trim();
        match token.token_type() {
            TokenType::Identifier { identifier } if !after_access => {
                out.insert(identifier.to_string());
            }
            TokenType::Symbol { .. } if text == "..." => {
                out.insert("...".into());
            }
            _ => {}
        }
        after_access = text == "." || text == ":";
    }
    out
}

/// Whether a block always leaves: `return`, `break`, `continue`, or a
/// final `error(...)` / `:Kick()`.
fn exits(block: &Block) -> bool {
    if block.last_stmt().is_some() {
        return true;
    }
    match block.stmts().last() {
        Some(Stmt::FunctionCall(call)) => {
            matches!(call.prefix(), Prefix::Name(n) if ident(n) == "error")
                || call.suffixes().any(
                    |s| matches!(s, Suffix::Call(Call::MethodCall(m)) if ident(m.name()) == "Kick"),
                )
        }
        _ => false,
    }
}
