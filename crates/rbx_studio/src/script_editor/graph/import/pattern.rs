//! Which catalog kind writes a piece of source. Each kind's template is
//! parsed once into a pattern; a source expression or statement matches
//! when it has the pattern's shape and the same literal text, and what sits
//! in the template's holes is what the kind's pins take.

use std::sync::OnceLock;

use super::super::catalog::{self, Code, Kind};
use super::tree::{snippet, Style, E, F, K, SK, S};

/// A match: the kind, and what the source holds where its template has
/// holes (`{Pin}` takes a whole expression, `{.Pin}` a field name).
pub(super) struct Hit<'e> {
    pub(super) kind: &'static Kind,
    pub(super) holes: Vec<(String, &'e E)>,
    pub(super) words: Vec<(String, String)>,
}

enum Shape {
    Value(E),
    Call(E),
    Assign(E, E),
}

struct Pattern {
    kind: &'static Kind,
    src: String,
    shape: Shape,
}

/// `{Pin}` as `__H_Pin` and `{.Pin}` as `.__W_Pin`, so the template parses.
fn holed(template: &str) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('}') else {
            break;
        };
        let pin = &rest[open + 1..open + close];
        match pin.strip_prefix('.') {
            Some(pin) => out.push_str(&format!(".__W_{pin}")),
            None => out.push_str(&format!("__H_{pin}")),
        }
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    out
}

fn parse(kind: &'static Kind, template: &str, value: bool) -> Option<Pattern> {
    let text = holed(template);
    let src = if value { format!("local _ = {text}") } else { text };
    let tree = snippet(&src)?;
    let shape = match &tree.block.items.first()? {
        super::tree::Item::S(S { k: SK::Local { vals, .. }, .. }) if value => {
            Shape::Value(vals.first()?.clone())
        }
        super::tree::Item::S(S { k: SK::Call(e), .. }) if !value => Shape::Call(e.clone()),
        super::tree::Item::S(S { k: SK::Assign(t, v), .. }) if !value => {
            Shape::Assign(t.first()?.clone(), v.first()?.clone())
        }
        _ => return None,
    };
    Some(Pattern { kind, src, shape })
}

fn patterns() -> &'static [Pattern] {
    static ALL: OnceLock<Vec<Pattern>> = OnceLock::new();
    ALL.get_or_init(|| {
        catalog::all()
            .filter_map(|kind| match kind.code {
                Code::Event(Some(t)) => parse(kind, t, true),
                Code::Statement(t) => parse(kind, t, false),
                // A template that is only a hole would match everything.
                Code::Expression { template, .. } if !(template.matches('{').count() == 1 && template.starts_with('{') && template.ends_with('}')) => {
                    parse(kind, template, true)
                }
                _ => None,
            })
            .collect()
    })
}

fn same_shape<'e>(p: &E, ps: &str, e: &'e E, src: &str, hit: &mut Hit<'e>) -> bool {
    let all = |ps_list: &[E], list: &'e [E], hit: &mut Hit<'e>| {
        ps_list.len() == list.len()
            && ps_list.iter().zip(list).all(|(p, e)| same_shape(p, ps, e, src, hit))
    };
    match (&p.k, &e.k) {
        (K::Name(n), _) if n.starts_with("__H_") => {
            hit.holes.push((n[4..].to_owned(), e));
            true
        }
        (K::Name(a), K::Name(b)) => a == b,
        (K::Lit, K::Lit) => ps[p.lo..p.hi] == src[e.lo..e.hi],
        (K::Field(pb, pn), K::Field(b, n)) => {
            match pn.strip_prefix("__W_") {
                Some(pin) => hit.words.push((pin.to_owned(), n.clone())),
                None if pn != n => return false,
                None => {}
            }
            same_shape(pb, ps, b, src, hit)
        }
        (K::Index(pb, pk), K::Index(b, k)) => {
            same_shape(pb, ps, b, src, hit) && same_shape(pk, ps, k, src, hit)
        }
        (K::Call(pf, pa, Style::Paren), K::Call(f, a, Style::Paren)) => {
            same_shape(pf, ps, f, src, hit) && all(pa, a, hit)
        }
        (K::Method(po, pn, pa, Style::Paren), K::Method(o, n, a, Style::Paren)) => {
            pn == n && same_shape(po, ps, o, src, hit) && all(pa, a, hit)
        }
        (K::Bin(pa, po, pb), K::Bin(a, o, b)) => {
            po == o && same_shape(pa, ps, a, src, hit) && same_shape(pb, ps, b, src, hit)
        }
        (K::Un(po, pa), K::Un(o, a)) => po == o && same_shape(pa, ps, a, src, hit),
        (K::Paren(pa), K::Paren(a)) => same_shape(pa, ps, a, src, hit),
        (K::IfExp(parms, pelse), K::IfExp(arms, els)) => {
            parms.len() == arms.len()
                && parms.iter().zip(arms).all(|((pc, pt), (c, t))| {
                    same_shape(pc, ps, c, src, hit) && same_shape(pt, ps, t, src, hit)
                })
                && same_shape(pelse, ps, els, src, hit)
        }
        _ => false,
    }
}

fn hit<'e>(kind: &'static Kind) -> Hit<'e> {
    Hit {
        kind,
        holes: Vec::new(),
        words: Vec::new(),
    }
}

/// Every value kind whose template reads as `e`, in catalog order.
pub(super) fn expression<'e>(src: &str, e: &'e E) -> Vec<Hit<'e>> {
    patterns()
        .iter()
        .filter(|p| matches!(p.kind.code, Code::Expression { .. }))
        .filter_map(|p| {
            let Shape::Value(shape) = &p.shape else {
                return None;
            };
            let mut h = hit(p.kind);
            same_shape(shape, &p.src, e, src, &mut h).then_some(h)
        })
        .collect()
}

/// Every statement kind whose template reads as `s`, in catalog order.
pub(super) fn statement<'e>(src: &str, s: &'e S) -> Vec<Hit<'e>> {
    patterns()
        .iter()
        .filter(|p| matches!(p.kind.code, Code::Statement(_)))
        .filter_map(|p| {
            let mut h = hit(p.kind);
            let ok = match (&p.shape, &s.k) {
                (Shape::Call(shape), SK::Call(e)) => same_shape(shape, &p.src, e, src, &mut h),
                (Shape::Assign(pt, pv), SK::Assign(ts, vs)) if ts.len() == 1 && vs.len() == 1 => {
                    same_shape(pt, &p.src, &ts[0], src, &mut h)
                        && same_shape(pv, &p.src, &vs[0], src, &mut h)
                }
                _ => false,
            };
            ok.then_some(h)
        })
        .collect()
}

/// `SIGNAL:Connect(function(params) ... end)` where SIGNAL is an event's
/// template and the handler is a plain one the event can pass its values to.
pub(super) fn event<'e>(src: &str, call: &'e E) -> Option<(Hit<'e>, &'e F)> {
    let K::Method(signal, name, args, Style::Paren) = &call.k else {
        return None;
    };
    let [E { k: K::Func(f), .. }] = args.as_slice() else {
        return None;
    };
    if name != "Connect"
        || !f.attrs.is_empty()
        || !f.generics.is_empty()
        || !f.returns.is_empty()
        || f.typed
        || f.vararg
    {
        return None;
    }
    patterns()
        .iter()
        .filter(|p| matches!(p.kind.code, Code::Event(Some(_))))
        .find_map(|p| {
            let Shape::Value(shape) = &p.shape else {
                return None;
            };
            let mut h = hit(p.kind);
            let fits = same_shape(shape, &p.src, signal, src, &mut h);
            let params = p.kind.outputs.iter().filter(|o| o.ty != catalog::PinType::Exec).count();
            (fits && f.names.len() <= params).then_some((h, &**f))
        })
}
