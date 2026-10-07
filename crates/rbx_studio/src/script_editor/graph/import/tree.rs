//! The parsed script in the shape the importer wants: full_moon's syntax
//! tree with every node's byte span in the source, and the comments that sit
//! between statements pulled out as items of their blocks. Text the graph
//! keeps verbatim (type annotations, parameter lists) is a slice of the
//! source, never re-printed.

use std::cell::RefCell;
use std::collections::HashSet;

use full_moon::ast::{self, Expression, Stmt};
use full_moon::node::Node;
use full_moon::tokenizer::{TokenReference, TokenType};

#[derive(Debug, Clone)]
pub(super) struct E {
    pub(super) k: K,
    pub(super) lo: usize,
    pub(super) hi: usize,
}

/// How a call's arguments were written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Style {
    Paren,
    Str,
    Table,
}

#[derive(Debug, Clone)]
pub(super) enum K {
    Name(String),
    /// A number, string or `nil`/`true`/`false`/`...`: its text is the span.
    Lit,
    Field(Box<E>, String),
    Index(Box<E>, Box<E>),
    Call(Box<E>, Vec<E>, Style),
    Method(Box<E>, String, Vec<E>, Style),
    Bin(Box<E>, String, Box<E>),
    Un(String, Box<E>),
    Paren(Box<E>),
    Func(Box<F>),
    Table(Vec<Fld>),
    IfExp(Vec<(E, E)>, Box<E>),
    /// The text between the interpolation holes, then what is in them.
    Interp(Vec<String>, Vec<E>),
    Cast(Box<E>, String),
    /// Syntax the graph has no node for; kept as its own text.
    Bad,
}

#[derive(Debug, Clone)]
pub(super) enum Key {
    None,
    Name(String),
    Expr(E),
}

#[derive(Debug, Clone)]
pub(super) struct Fld {
    pub(super) key: Key,
    pub(super) value: E,
    /// Where the field starts, key included.
    pub(super) lo: usize,
    /// The `,` or `;` after it, if there is one.
    pub(super) sep: String,
}

#[derive(Debug, Clone)]
pub(super) struct F {
    pub(super) attrs: String,
    pub(super) generics: String,
    /// Between the parentheses, as written.
    pub(super) params: String,
    pub(super) names: Vec<String>,
    pub(super) vararg: bool,
    pub(super) typed: bool,
    /// The return annotation with its colon.
    pub(super) returns: String,
    pub(super) body: Block,
}

#[derive(Debug, Clone)]
pub(super) struct Block {
    pub(super) items: Vec<Item>,
    /// The span the block's text may occupy: its statements and the comments
    /// between them lie inside it.
    pub(super) lo: usize,
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)] // a handful per block; boxing buys nothing
pub(super) enum Item {
    S(S),
    C(Cm),
}

impl Item {
    pub(super) fn span(&self) -> (usize, usize) {
        match self {
            Item::S(s) => (s.lo, s.hi),
            Item::C(c) => (c.lo, c.hi),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Cm {
    pub(super) lo: usize,
    pub(super) hi: usize,
    /// Written on the line of what came before it.
    pub(super) inline: bool,
}

#[derive(Debug, Clone)]
pub(super) struct S {
    pub(super) k: SK,
    pub(super) lo: usize,
    /// Past a trailing `;`.
    pub(super) hi: usize,
}

#[derive(Debug, Clone)]
pub(super) enum SK {
    Local {
        names: Vec<String>,
        /// Names with their annotations, as written.
        text: String,
        typed: bool,
        vals: Vec<E>,
    },
    Assign(Vec<E>, Vec<E>),
    Compound(E, String, E),
    Call(E),
    If(Vec<(E, Block)>, Option<Block>),
    While(E, Block),
    Repeat(Block, E),
    ForCount {
        var: String,
        name: String,
        from: E,
        to: E,
        step: Option<E>,
        body: Block,
    },
    ForEach {
        vars: String,
        names: Vec<String>,
        vals: Vec<E>,
        body: Block,
    },
    Do(Block),
    Function {
        attrs: String,
        local: bool,
        name: String,
        /// The first name of `a.b:c`: what the declaration assigns to.
        root: String,
        f: F,
    },
    Return(Vec<E>),
    Break,
    Continue,
    Type,
    Bad,
}

pub(super) struct Tree {
    pub(super) block: Block,
    /// Every comment, sorted by start.
    pub(super) comments: Vec<Cm>,
    /// The starts of those that became items of a block.
    pub(super) claimed: HashSet<usize>,
    /// Every real token as (start, end), sorted.
    pub(super) tokens: Vec<(usize, usize)>,
}

fn pos(p: Option<full_moon::tokenizer::Position>, default: usize) -> usize {
    p.map_or(default, |p| p.bytes())
}

fn lo_of<N: Node>(n: &N) -> usize {
    pos(n.start_position(), 0)
}

/// The node's own end can stop short of a closing brace in a table type, so
/// the last of its tokens counts too.
fn hi_of<N: Node>(n: &N) -> usize {
    n.tokens()
        .map(tok_hi)
        .fold(pos(n.end_position(), 0), usize::max)
}

fn tok_lo(t: &TokenReference) -> usize {
    t.token().start_position().bytes()
}

fn tok_hi(t: &TokenReference) -> usize {
    t.token().end_position().bytes()
}

struct Cx<'a> {
    src: &'a str,
    comments: &'a [Cm],
    claimed: RefCell<HashSet<usize>>,
}

pub(super) fn tree(src: &str, ast: &ast::Ast) -> Tree {
    let mut tokens: Vec<(usize, usize)> = ast
        .nodes()
        .tokens()
        .map(|t| (tok_lo(t), tok_hi(t)))
        .collect();
    tokens.sort_unstable();
    let mut comments = Vec::new();
    let mut seen = |t: &TokenReference| {
        for trivia in t.leading_trivia().chain(t.trailing_trivia()) {
            if matches!(
                trivia.token_type(),
                TokenType::SingleLineComment { .. }
                    | TokenType::MultiLineComment { .. }
                    | TokenType::Shebang { .. }
            ) {
                let lo = trivia.start_position().bytes();
                let mut hi = trivia.end_position().bytes();
                while hi > lo && src.as_bytes()[hi - 1] == b'\r' {
                    hi -= 1;
                }
                comments.push(Cm {
                    lo,
                    hi,
                    inline: false,
                });
            }
        }
    };
    for t in ast.nodes().tokens() {
        seen(t);
    }
    seen(ast.eof());
    comments.sort_by_key(|c| c.lo);
    comments.dedup_by_key(|c| c.lo);
    let cx = Cx {
        src,
        comments: &comments,
        claimed: RefCell::default(),
    };
    let mut block = cx.block(ast.nodes(), 0, src.len());
    // A shebang or a first comment is never "inline" with anything.
    if let Some(Item::C(c)) = block.items.first_mut() {
        c.inline = false;
    }
    let claimed = cx.claimed.into_inner();
    Tree {
        block,
        comments,
        claimed,
        tokens,
    }
}

/// A piece of Luau (a catalog template) as a tree.
pub(super) fn snippet(code: &str) -> Option<Tree> {
    let ast = full_moon::parse(code).ok()?;
    Some(tree(code, &ast))
}

impl Cx<'_> {
    fn text(&self, lo: usize, hi: usize) -> String {
        self.src.get(lo..hi).unwrap_or_default().to_owned()
    }

    /// The statements of a block with the comments between them, in order.
    /// `lo..hi` is the room the block has: from the token before it to the
    /// token after.
    fn block(&self, b: &ast::Block, lo: usize, hi: usize) -> Block {
        let mut stmts: Vec<S> = b
            .stmts_with_semicolon()
            .map(|(s, semi)| self.stmt(s, semi.as_ref()))
            .collect();
        if let Some((last, semi)) = b.last_stmt_with_semicolon() {
            stmts.push(self.last(last, semi.as_ref()));
        }
        let mut items = Vec::new();
        let mut cursor = lo;
        let mut comments = self
            .comments
            .iter()
            .filter(|c| c.lo >= lo && c.hi <= hi)
            .peekable();
        let place = |items: &mut Vec<Item>, comment: &Cm, cursor: usize| {
            let inline = !self.src[cursor..comment.lo].contains('\n') && cursor > 0;
            self.claimed.borrow_mut().insert(comment.lo);
            items.push(Item::C(Cm { inline, ..*comment }));
        };
        for s in stmts {
            while let Some(c) = comments.next_if(|c| c.hi <= s.lo) {
                place(&mut items, c, cursor);
                cursor = c.hi;
            }
            // What is inside the statement is its own.
            while comments.next_if(|c| c.lo < s.hi).is_some() {}
            cursor = s.hi;
            items.push(Item::S(s));
        }
        for c in comments {
            place(&mut items, c, cursor);
            cursor = c.hi;
        }
        Block { items, lo }
    }

    fn last(&self, s: &ast::LastStmt, semi: Option<&TokenReference>) -> S {
        let lo = lo_of(s);
        let hi = semi.map_or_else(|| hi_of(s), tok_hi);
        let k = match s {
            ast::LastStmt::Break(_) => SK::Break,
            ast::LastStmt::Continue(_) => SK::Continue,
            ast::LastStmt::Return(r) => {
                SK::Return(r.returns().iter().map(|e| self.expr(e)).collect())
            }
            _ => SK::Bad,
        };
        S { k, lo, hi }
    }

    fn stmt(&self, s: &Stmt, semi: Option<&TokenReference>) -> S {
        let lo = lo_of(s);
        let hi = semi.map_or_else(|| hi_of(s), tok_hi);
        let k = self.stmt_kind(s);
        S { k, lo, hi }
    }

    fn stmt_kind(&self, s: &Stmt) -> SK {
        match s {
            Stmt::LocalAssignment(a) => {
                let names: Vec<String> = a.names().iter().map(|t| t.token().to_string()).collect();
                let first = a.names().first().map_or(0, |p| tok_lo(p.value()));
                let end = a.equal_token().map_or_else(|| hi_of(a), tok_lo);
                SK::Local {
                    names,
                    text: self.text(first, end).trim_end().to_owned(),
                    typed: a.type_specifiers().any(|t| t.is_some()),
                    vals: a.expressions().iter().map(|e| self.expr(e)).collect(),
                }
            }
            Stmt::Assignment(a) => SK::Assign(
                a.variables().iter().map(|v| self.var(v)).collect(),
                a.expressions().iter().map(|e| self.expr(e)).collect(),
            ),
            Stmt::CompoundAssignment(c) => SK::Compound(
                self.var(c.lhs()),
                self.text(lo_of(c.compound_operator()), hi_of(c.compound_operator())),
                self.expr(c.rhs()),
            ),
            Stmt::FunctionCall(c) => SK::Call(self.call(c)),
            Stmt::If(i) => {
                if i.binding().is_some()
                    || i.else_if()
                        .is_some_and(|e| e.iter().any(|e| e.binding().is_some()))
                {
                    return SK::Bad;
                }
                let mut arms = Vec::new();
                let mut after = i.else_token().map_or_else(|| tok_lo(i.end_token()), tok_lo);
                let elseifs: &[ast::ElseIf] = i.else_if().map_or(&[], |v| v.as_slice());
                if let Some(first) = elseifs.first() {
                    after = tok_lo(first.else_if_token());
                }
                arms.push((
                    self.expr(i.condition()),
                    self.block(i.block(), tok_hi(i.then_token()), after),
                ));
                for (n, e) in elseifs.iter().enumerate() {
                    let next = match elseifs.get(n + 1) {
                        Some(next) => tok_lo(next.else_if_token()),
                        None => i.else_token().map_or_else(|| tok_lo(i.end_token()), tok_lo),
                    };
                    arms.push((
                        self.expr(e.condition()),
                        self.block(e.block(), tok_hi(e.then_token()), next),
                    ));
                }
                let els = i.else_block().map(|b| {
                    let from = i.else_token().map_or(0, tok_hi);
                    self.block(b, from, tok_lo(i.end_token()))
                });
                SK::If(arms, els)
            }
            Stmt::While(w) => SK::While(
                self.expr(w.condition()),
                self.block(w.block(), tok_hi(w.do_token()), tok_lo(w.end_token())),
            ),
            Stmt::Repeat(r) => SK::Repeat(
                self.block(r.block(), tok_hi(r.repeat_token()), tok_lo(r.until_token())),
                self.expr(r.until()),
            ),
            Stmt::NumericFor(f) => SK::ForCount {
                var: self
                    .text(tok_lo(f.index_variable()), tok_lo(f.equal_token()))
                    .trim_end()
                    .to_owned(),
                name: f.index_variable().token().to_string(),
                from: self.expr(f.start()),
                to: self.expr(f.end()),
                step: f.step().map(|e| self.expr(e)),
                body: self.block(f.block(), tok_hi(f.do_token()), tok_lo(f.end_token())),
            },
            Stmt::GenericFor(f) => {
                let first = f.names().first().map_or(0, |p| tok_lo(p.value()));
                SK::ForEach {
                    vars: self.text(first, tok_lo(f.in_token())).trim_end().to_owned(),
                    names: f.names().iter().map(|t| t.token().to_string()).collect(),
                    vals: f.expressions().iter().map(|e| self.expr(e)).collect(),
                    body: self.block(f.block(), tok_hi(f.do_token()), tok_lo(f.end_token())),
                }
            }
            Stmt::Do(d) => {
                SK::Do(self.block(d.block(), tok_hi(d.do_token()), tok_lo(d.end_token())))
            }
            Stmt::FunctionDeclaration(d) => {
                let name = d.name();
                SK::Function {
                    attrs: self.attrs(d.attributes()),
                    local: false,
                    name: self.text(lo_of(name), hi_of(name)),
                    root: name
                        .names()
                        .first()
                        .map_or_else(String::new, |p| p.value().token().to_string()),
                    f: self.body(d.body()),
                }
            }
            Stmt::LocalFunction(d) => SK::Function {
                attrs: self.attrs(d.attributes()),
                local: true,
                name: d.name().token().to_string(),
                root: d.name().token().to_string(),
                f: self.body(d.body()),
            },
            Stmt::TypeDeclaration(_)
            | Stmt::ExportedTypeDeclaration(_)
            | Stmt::TypeFunction(_)
            | Stmt::ExportedTypeFunction(_) => SK::Type,
            _ => SK::Bad,
        }
    }

    fn attrs<'x>(&self, attrs: impl Iterator<Item = &'x ast::luau::LuauAttribute>) -> String {
        let all: Vec<_> = attrs.collect();
        match (all.first(), all.last()) {
            (Some(a), Some(b)) => self.text(lo_of(*a), hi_of(*b)),
            _ => String::new(),
        }
    }

    fn body(&self, b: &ast::FunctionBody) -> F {
        let parens = b.parameters_parentheses().tokens();
        let (open, close) = (tok_hi(parens.0), tok_lo(parens.1));
        let after = b.return_type().map_or(close + 1, hi_of);
        let mut names = Vec::new();
        let mut vararg = false;
        for p in b.parameters() {
            match p {
                ast::Parameter::Name(t) => names.push(t.token().to_string()),
                _ => vararg = true,
            }
        }
        F {
            attrs: String::new(),
            generics: b
                .generics()
                .map(|g| self.text(lo_of(g), hi_of(g)))
                .unwrap_or_default(),
            params: self.text(open, close),
            names,
            vararg,
            typed: b.type_specifiers().any(|t| t.is_some()),
            returns: b
                .return_type()
                .map(|r| self.text(lo_of(r), hi_of(r)))
                .unwrap_or_default(),
            body: self.block(b.block(), after.max(close + 1), tok_lo(b.end_token())),
        }
    }

    fn var(&self, v: &ast::Var) -> E {
        match v {
            ast::Var::Name(t) => E {
                k: K::Name(t.token().to_string()),
                lo: tok_lo(t),
                hi: tok_hi(t),
            },
            ast::Var::Expression(v) => self.chain(
                v.prefix(),
                v.suffixes().collect(),
                lo_of(v.as_ref()),
                hi_of(v.as_ref()),
            ),
            _ => self.bad(v),
        }
    }

    fn bad<N: Node>(&self, n: &N) -> E {
        E {
            k: K::Bad,
            lo: lo_of(n),
            hi: hi_of(n),
        }
    }

    fn call(&self, c: &ast::FunctionCall) -> E {
        self.chain(c.prefix(), c.suffixes().collect(), lo_of(c), hi_of(c))
    }

    /// `a.b[c](d):e(f)` folds left into one node per suffix.
    fn chain(&self, prefix: &ast::Prefix, suffixes: Vec<&ast::Suffix>, lo: usize, hi: usize) -> E {
        let mut cur = match prefix {
            ast::Prefix::Name(t) => E {
                k: K::Name(t.token().to_string()),
                lo: tok_lo(t),
                hi: tok_hi(t),
            },
            ast::Prefix::Expression(e) => self.expr(e),
            _ => return E { k: K::Bad, lo, hi },
        };
        cur.lo = lo;
        for s in suffixes {
            let end = hi_of(s);
            let base = Box::new(cur);
            let k = match s {
                ast::Suffix::Index(ast::Index::Dot { name, .. }) => {
                    K::Field(base, name.token().to_string())
                }
                ast::Suffix::Index(ast::Index::Brackets { expression, .. }) => {
                    K::Index(base, Box::new(self.expr(expression)))
                }
                ast::Suffix::Call(ast::Call::AnonymousCall(args)) => match self.args(args) {
                    Some((a, st)) => K::Call(base, a, st),
                    None => return E { k: K::Bad, lo, hi },
                },
                ast::Suffix::Call(ast::Call::MethodCall(m)) => {
                    if m.type_instantiation().is_some() {
                        return E { k: K::Bad, lo, hi };
                    }
                    match self.args(m.args()) {
                        Some((a, st)) => K::Method(base, m.name().token().to_string(), a, st),
                        None => return E { k: K::Bad, lo, hi },
                    }
                }
                _ => return E { k: K::Bad, lo, hi },
            };
            cur = E { k, lo, hi: end };
        }
        cur.hi = hi;
        cur
    }

    fn args(&self, a: &ast::FunctionArgs) -> Option<(Vec<E>, Style)> {
        match a {
            ast::FunctionArgs::Parentheses { arguments, .. } => Some((
                arguments.iter().map(|e| self.expr(e)).collect(),
                Style::Paren,
            )),
            ast::FunctionArgs::String(t) => {
                let text = self.text(tok_lo(t), tok_hi(t));
                // Anything else would not print back bare.
                if !(text.starts_with(['"', '\'']) || text.starts_with("[[")) {
                    return None;
                }
                let e = E {
                    k: K::Lit,
                    lo: tok_lo(t),
                    hi: tok_hi(t),
                };
                Some((vec![e], Style::Str))
            }
            ast::FunctionArgs::TableConstructor(t) => Some((vec![self.table(t)], Style::Table)),
            _ => None,
        }
    }

    fn table(&self, t: &ast::TableConstructor) -> E {
        let fields = t
            .fields()
            .pairs()
            .map(|pair| {
                let (f, sep) = (pair.value(), pair.punctuation());
                let sep = sep.map(|p| p.token().to_string()).unwrap_or_default();
                match f {
                    ast::Field::ExpressionKey { key, value, .. } => Fld {
                        key: Key::Expr(self.expr(key)),
                        value: self.expr(value),
                        lo: lo_of(f),
                        sep,
                    },
                    ast::Field::NameKey { key, value, .. } => Fld {
                        key: Key::Name(key.token().to_string()),
                        value: self.expr(value),
                        lo: lo_of(f),
                        sep,
                    },
                    ast::Field::NoKey(value) => Fld {
                        key: Key::None,
                        value: self.expr(value),
                        lo: lo_of(f),
                        sep,
                    },
                    _ => Fld {
                        key: Key::None,
                        value: self.bad(f),
                        lo: lo_of(f),
                        sep,
                    },
                }
            })
            .collect();
        E {
            k: K::Table(fields),
            lo: lo_of(t),
            hi: hi_of(t),
        }
    }

    fn expr(&self, e: &Expression) -> E {
        let (lo, hi) = (lo_of(e), hi_of(e));
        let k = match e {
            Expression::BinaryOperator { lhs, binop, rhs } => K::Bin(
                Box::new(self.expr(lhs)),
                self.text(lo_of(binop), hi_of(binop)),
                Box::new(self.expr(rhs)),
            ),
            Expression::UnaryOperator { unop, expression } => K::Un(
                self.text(lo_of(unop), hi_of(unop)),
                Box::new(self.expr(expression)),
            ),
            Expression::Parentheses { expression, .. } => K::Paren(Box::new(self.expr(expression))),
            Expression::Function(f) => {
                let mut func = self.body(f.body());
                func.attrs = self.attrs(f.attributes());
                K::Func(Box::new(func))
            }
            Expression::FunctionCall(c) => return self.call(c),
            Expression::Var(v) => return self.var(v),
            Expression::Number(_) | Expression::String(_) | Expression::Symbol(_) => K::Lit,
            Expression::TypeAssertion {
                expression,
                type_assertion,
            } => K::Cast(
                Box::new(self.expr(expression)),
                self.text(
                    lo_of(type_assertion.cast_to()),
                    hi_of(type_assertion.cast_to()),
                )
                .trim()
                .to_owned(),
            ),
            Expression::TableConstructor(t) => return self.table(t),
            Expression::IfExpression(i) => {
                if i.binding().is_some()
                    || i.else_if_expressions()
                        .is_some_and(|v| v.iter().any(|e| e.binding().is_some()))
                {
                    K::Bad
                } else {
                    let mut arms = vec![(self.expr(i.condition()), self.expr(i.if_expression()))];
                    for e in i.else_if_expressions().into_iter().flatten() {
                        arms.push((self.expr(e.condition()), self.expr(e.expression())));
                    }
                    K::IfExp(arms, Box::new(self.expr(i.else_expression())))
                }
            }
            Expression::InterpolatedString(s) => {
                let mut segs = Vec::new();
                let mut exprs = Vec::new();
                let inner =
                    |t: &TokenReference| self.text(tok_lo(t) + 1, tok_hi(t).saturating_sub(1));
                for seg in s.segments() {
                    segs.push(inner(&seg.literal));
                    exprs.push(self.expr(&seg.expression));
                }
                segs.push(inner(s.last_string()));
                K::Interp(segs, exprs)
            }
            _ => K::Bad,
        };
        E { k, lo, hi }
    }
}
