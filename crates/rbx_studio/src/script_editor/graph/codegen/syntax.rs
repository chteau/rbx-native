//! What the generic syntax kinds write: one place per construct, so a
//! graph built from them prints as the code it was imported from.

use super::*;

/// The precedence of a binary operator typed on a node. An operator this
/// does not know groups loosest, so its operands are never bracketed away.
fn op_prec(op: &str) -> Prec {
    match op {
        "or" => Prec::Or,
        "and" => Prec::And,
        "==" | "~=" | "<" | ">" | "<=" | ">=" => Prec::Compare,
        ".." => Prec::Concat,
        "+" | "-" => Prec::Add,
        "*" | "/" | "//" | "%" => Prec::Mul,
        "^" => Prec::Pow,
        _ => Prec::Or,
    }
}

/// An expression that is called or indexed, bracketed if it could not be
/// written bare.
fn callee(text: String, prec: Prec) -> String {
    let needs = prec != Prec::Atom || !indexable(&text);
    wrap(text, needs)
}

impl Compiler<'_> {
    /// The code text typed on a node's text pin.
    fn word(&self, node: NodeId, pin: &str) -> String {
        self.graph.value(&End::new(node, pin)).unwrap_or_default()
    }

    /// A hidden value; empty when the node has none.
    fn hidden(&self, node: NodeId, key: &str) -> String {
        self.graph
            .node(node)
            .and_then(|node| node.values.get(key))
            .cloned()
            .unwrap_or_default()
    }

    fn count(&self, node: NodeId, key: &str) -> usize {
        let values = self.graph.node(node).map(|node| &node.values);
        let repeat = self.kind(node).repeats.iter().find(|r| r.count == key);
        match (repeat, values) {
            (Some(repeat), Some(values)) => repeat.count_in(values),
            _ => 0,
        }
    }

    /// Every instance of a repeated input, each reporting its own problem.
    fn many(
        &mut self,
        node: NodeId,
        base: &str,
        count: usize,
        scope: &mut Scope,
    ) -> Option<Vec<String>> {
        let all: Vec<_> = (1..=count)
            .map(|i| self.input(node, &format!("{base} {i}"), scope))
            .collect();
        all.into_iter()
            .map(|value| value.map(|(text, _)| text))
            .collect()
    }

    /// `(a, b)`, or the bare string or table of `f"x"` and `f{...}`.
    fn call_args(&mut self, node: NodeId, scope: &mut Scope) -> Option<String> {
        let count = self.count(node, "#args");
        let mut args = self.many(node, "Input", count, scope)?;
        let bare = matches!(self.hidden(node, "@style").as_str(), "string" | "table");
        if bare && args.len() == 1 && args[0].starts_with(['"', '\'', '{']) {
            return Some(args.remove(0));
        }
        if bare && args.len() == 1 && args[0].starts_with("[[") {
            return Some(args.remove(0));
        }
        Some(format!("({})", args.join(", ")))
    }

    /// `function` or `local function`, with attributes and generics.
    fn function_head(&self, node: NodeId) -> (String, String) {
        let attributes = self.hidden(node, "@attributes");
        let attributes = match attributes.is_empty() {
            true => String::new(),
            false => format!("{attributes} "),
        };
        let local = match self.hidden(node, "@local") == "1" {
            true => "local ",
            false => "",
        };
        let generics = self.hidden(node, "@generics");
        let generics = match generics.is_empty() || generics.starts_with('<') {
            true => generics,
            false => format!("<{generics}>"),
        };
        (format!("{attributes}{local}function"), generics)
    }

    /// An exec run written as the body of a value, at one level deeper.
    fn inline_body(&mut self, from: &End, scope: &Scope) -> String {
        let saved = std::mem::take(&mut self.out);
        let lead = self.lead.take();
        let mut inner = scope.clone();
        self.indent += 1;
        self.run(from, &mut inner);
        self.indent -= 1;
        self.lead = lead;
        std::mem::replace(&mut self.out, saved)
    }

    /// Writes a statement of a generic kind.
    pub(super) fn syntax(&mut self, node: NodeId, syn: Syn, scope: &mut Scope) {
        let exec = |pin: &str| End::new(node, pin);
        match syn {
            Syn::Local => {
                let count = self.count(node, "#values");
                let Some(values) = self.many(node, "Value", count, scope) else {
                    return;
                };
                let names = self.word(node, "Names");
                match values.is_empty() {
                    true => self.put(format!("local {names}")),
                    false => self.put(format!("local {names} = {}", values.join(", "))),
                }
            }
            Syn::Assign => {
                let (targets, values) = (self.count(node, "#targets"), self.count(node, "#values"));
                let targets = self.many(node, "Target", targets, scope);
                let values = self.many(node, "Value", values, scope);
                if let (Some(targets), Some(values)) = (targets, values) {
                    self.put(format!("{} = {}", targets.join(", "), values.join(", ")));
                }
            }
            Syn::Compound => {
                let target = self.input(node, "Target", scope);
                let value = self.input(node, "Value", scope);
                if let (Some((target, _)), Some((value, _))) = (target, value) {
                    let op = self.word(node, "Op");
                    self.put(format!("{target} {} {value}", op.trim()));
                }
            }
            Syn::Call => {
                let function = self.input(node, "Function", scope);
                let args = self.call_args(node, scope);
                if let (Some((function, prec)), Some(args)) = (function, args) {
                    self.put(format!("{}{args}", callee(function, prec)));
                }
            }
            Syn::Method => {
                let object = self.input(node, "Object", scope);
                let args = self.call_args(node, scope);
                if let (Some((object, prec)), Some(args)) = (object, args) {
                    let method = self.word(node, "Method");
                    self.put(format!("{}:{method}{args}", callee(object, prec)));
                }
            }
            Syn::If => self.if_statement(node, scope),
            Syn::While => {
                if let Some((condition, _)) = self.input(node, "Condition", scope) {
                    self.put(format!("while {condition} do"));
                    self.body(&exec("Do"), scope.clone());
                }
            }
            Syn::RepeatUntil => {
                self.put("repeat");
                let mut inner = scope.clone();
                self.indent += 1;
                self.run(&exec("Do"), &mut inner);
                // The condition sees the body's locals, so its shared
                // values are read in there too.
                if !self.elide {
                    self.hoist_pin(node, "Condition", &mut inner);
                }
                self.indent -= 1;
                if let Some((condition, _)) = self.input(node, "Condition", &mut inner) {
                    self.put(format!("until {condition}"));
                }
            }
            Syn::ForCount => {
                let from = self.input(node, "From", scope);
                let to = self.input(node, "To", scope);
                let step = match self.graph.wire_into(&exec("Step")).is_none()
                    && self.word(node, "Step").trim().is_empty()
                {
                    true => Some(None),
                    false => self.input(node, "Step", scope).map(Some),
                };
                if let (Some((from, _)), Some((to, _)), Some(step)) = (from, to, step) {
                    let variable = self.word(node, "Variable");
                    let step = step.map_or(String::new(), |(step, _)| format!(", {step}"));
                    self.put(format!("for {variable} = {from}, {to}{step} do"));
                    self.body(&exec("Do"), scope.clone());
                }
            }
            Syn::ForIn => {
                let count = self.count(node, "#values");
                if let Some(values) = self.many(node, "In", count, scope) {
                    let variables = self.word(node, "Variables");
                    self.put(format!("for {variables} in {} do", values.join(", ")));
                    self.body(&exec("Do"), scope.clone());
                }
            }
            Syn::Do => {
                self.put("do");
                self.body(&exec("Do"), scope.clone());
            }
            Syn::Function => {
                let (head, generics) = self.function_head(node);
                let name = self.word(node, "Name");
                let params = self.word(node, "Parameters");
                let returns = self.hidden(node, "@returns");
                self.put(format!("{head} {name}{generics}({params}){returns}"));
                self.body(&exec("Body"), scope.clone());
            }
            Syn::Return => {
                let count = self.count(node, "#values");
                if let Some(values) = self.many(node, "Value", count, scope) {
                    match values.is_empty() {
                        true => self.put("return"),
                        false => self.put(format!("return {}", values.join(", "))),
                    }
                }
            }
            Syn::Break => self.put("break"),
            Syn::Continue => self.put("continue"),
            Syn::Type | Syn::Comment => {
                let text = self.word(node, "Text");
                self.put(text);
            }
            _ => {}
        }
    }

    fn if_statement(&mut self, node: NodeId, scope: &mut Scope) {
        let count = self.count(node, "#branches");
        let conditions = self.many(node, "Condition", count, scope);
        let Some(conditions) = conditions else {
            return;
        };
        for (i, condition) in conditions.iter().enumerate() {
            let word = if i == 0 { "if" } else { "elseif" };
            self.put(format!("{word} {condition} then"));
            self.block(&End::new(node, &format!("Then {}", i + 1)), scope);
        }
        let wired = self.graph.wires_from(&End::new(node, "Else")).next().is_some();
        if wired || self.hidden(node, "@else") == "1" {
            self.put("else");
            self.block(&End::new(node, "Else"), scope);
        }
        self.put("end");
    }

    /// An expression of a generic kind.
    pub(super) fn syntax_value(
        &mut self,
        node: NodeId,
        syn: Syn,
        scope: &mut Scope,
    ) -> Option<(String, Prec)> {
        match syn {
            Syn::Get => Some((self.word(node, "Name"), Prec::Atom)),
            Syn::Literal => {
                let text = self.word(node, "Text");
                let prec = match text.starts_with('-') {
                    true => Prec::Unary,
                    false => Prec::Atom,
                };
                Some((text, prec))
            }
            Syn::Field => {
                let (object, prec) = self.input(node, "Object", scope)?;
                let name = self.word(node, "Name");
                Some((format!("{}.{name}", callee(object, prec)), Prec::Atom))
            }
            Syn::Index => {
                let object = self.input(node, "Object", scope);
                let key = self.input(node, "Key", scope);
                let ((object, prec), (key, _)) = (object?, key?);
                Some((format!("{}[{key}]", callee(object, prec)), Prec::Atom))
            }
            Syn::CallValue => {
                let function = self.input(node, "Function", scope);
                let args = self.call_args(node, scope);
                let ((function, prec), args) = (function?, args?);
                Some((format!("{}{args}", callee(function, prec)), Prec::Atom))
            }
            Syn::MethodValue => {
                let object = self.input(node, "Object", scope);
                let args = self.call_args(node, scope);
                let ((object, prec), args) = (object?, args?);
                let method = self.word(node, "Method");
                Some((
                    format!("{}:{method}{args}", callee(object, prec)),
                    Prec::Atom,
                ))
            }
            Syn::Binary => {
                let a = self.input(node, "A", scope);
                let b = self.input(node, "B", scope);
                let ((a, pa), (b, pb)) = (a?, b?);
                let op = self.word(node, "Op");
                let op = op.trim();
                let parent = op_prec(op);
                let a = wrap(a, operand_needs(pa, parent, true));
                let b = wrap(b, operand_needs(pb, parent, false));
                Some((format!("{a} {op} {b}"), parent))
            }
            Syn::Unary => {
                let (value, prec) = self.input(node, "Value", scope)?;
                let op = self.word(node, "Op");
                let op = op.trim();
                let gap = match op.ends_with(|c: char| c.is_alphabetic()) {
                    true => " ",
                    false => "",
                };
                // `- -x` must not become a comment.
                let needs = prec < Prec::Unary || (op.ends_with('-') && value.starts_with('-'));
                Some((format!("{op}{gap}{}", wrap(value, needs)), Prec::Unary))
            }
            Syn::Paren => {
                let (value, _) = self.input(node, "Value", scope)?;
                Some((format!("({value})"), Prec::Atom))
            }
            Syn::Cast => {
                let (value, prec) = self.input(node, "Value", scope)?;
                let ty = self.word(node, "Type");
                Some((
                    format!("{} :: {}", wrap(value, prec < Prec::Cast), ty.trim()),
                    Prec::Cast,
                ))
            }
            Syn::FunctionValue => {
                let (head, generics) = self.function_head(node);
                let params = self.word(node, "Parameters");
                let returns = self.hidden(node, "@returns");
                let body = self.inline_body(&End::new(node, "Body"), scope);
                let tabs = "\t".repeat(self.indent);
                Some((
                    format!("{head}{generics}({params}){returns}{body}\n{tabs}end"),
                    Prec::Atom,
                ))
            }
            Syn::Table => self.table(node, scope),
            Syn::Pair => {
                let key = self.input(node, "Key", scope);
                let value = self.input(node, "Value", scope);
                let ((key, _), (value, _)) = (key?, value?);
                Some((format!("[{key}] = {value}"), Prec::Atom))
            }
            Syn::IfValue => {
                let count = self.count(node, "#branches");
                let conditions = self.many(node, "Condition", count, scope);
                let thens = self.many(node, "Then", count, scope);
                let otherwise = self.input(node, "Else", scope);
                let (conditions, thens, (otherwise, _)) = (conditions?, thens?, otherwise?);
                let mut text = String::new();
                for (i, (condition, then)) in conditions.iter().zip(&thens).enumerate() {
                    let word = if i == 0 { "if" } else { " elseif" };
                    text.push_str(&format!("{word} {condition} then {then}"));
                }
                text.push_str(&format!(" else {otherwise}"));
                Some((text, Prec::If))
            }
            Syn::Interp => {
                let count = self.count(node, "#parts");
                let values = self.many(node, "Value", count, scope)?;
                let mut text = format!("`{}", self.hidden(node, "@seg0"));
                for (i, value) in values.iter().enumerate() {
                    // `{{` would open an escape.
                    let gap = if value.starts_with('{') { " " } else { "" };
                    let seg = self.hidden(node, &format!("@seg{}", i + 1));
                    text.push_str(&format!("{{{gap}{value}}}{seg}"));
                }
                text.push('`');
                Some((text, Prec::Atom))
            }
            _ => None,
        }
    }

    fn table(&mut self, node: NodeId, scope: &mut Scope) -> Option<(String, Prec)> {
        let count = self.count(node, "#fields");
        let multiline = self.hidden(node, "@multiline") == "1";
        if multiline {
            self.indent += 1;
        }
        let items = self.many(node, "Item", count, scope);
        if multiline {
            self.indent -= 1;
        }
        let items = items?;
        let tabs = "\t".repeat(self.indent + 1);
        let mut text = String::from("{");
        for (i, item) in items.iter().enumerate() {
            let n = i + 1;
            let key = self.hidden(node, &format!("@key{n}"));
            let item = match key.is_empty() {
                true => item.clone(),
                false => format!("{key} = {item}"),
            };
            let sep = match self.graph.node(node).and_then(|node| node.values.get(&format!("@sep{n}"))) {
                Some(sep) => sep.clone(),
                None if n < items.len() => ",".into(),
                None => String::new(),
            };
            let lead = self.hidden(node, &format!("@lead{n}"));
            match multiline {
                true => {
                    text.push_str(&format!("\n{tabs}"));
                    if !lead.is_empty() {
                        text.push_str(&format!("{lead}\n{tabs}"));
                    }
                }
                false => {
                    if i > 0 {
                        text.push(' ');
                    }
                    if !lead.is_empty() {
                        text.push_str(&format!("{lead} "));
                    }
                }
            }
            text.push_str(&format!("{item}{sep}"));
        }
        if multiline {
            text.push_str(&format!("\n{}", "\t".repeat(self.indent)));
        }
        text.push('}');
        Some((text, Prec::Atom))
    }
}
