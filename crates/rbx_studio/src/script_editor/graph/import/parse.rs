//! Statements found in a token list: where each starts and ends, and for
//! the few the importer can draw (`if`, `for`) where their parts are. Every
//! other statement is only bounded, to be kept as written.

use super::lex::{Tok, T};

pub(super) struct P<'a> {
    pub(super) src: &'a str,
    pub(super) toks: &'a [Tok],
}

/// The statements between an opener and its closer; `lo` and `hi` are byte
/// offsets: the end of the opener and the start of the closer.
pub(super) struct Block {
    pub(super) stmts: Vec<Stmt>,
    pub(super) lo: usize,
    pub(super) hi: usize,
}

/// Token indices, `hi` exclusive.
pub(super) struct Stmt {
    pub(super) lo: usize,
    pub(super) hi: usize,
    pub(super) shape: Shape,
}

pub(super) enum Shape {
    /// An expression statement, an assignment or a `local`.
    Plain,
    If {
        cond: (usize, usize),
        yes: Block,
        no: Option<Block>,
    },
    /// `head` is everything between `for` and `do`.
    For {
        head: (usize, usize),
        body: Block,
    },
    Other,
}

const IF_ENDS: &[&str] = &["elseif", "else", "end"];

impl<'a> P<'a> {
    pub(super) fn text(&self, i: usize) -> &'a str {
        let tok = &self.toks[i];
        &self.src[tok.start..tok.end]
    }

    /// Whether token `i` is the keyword or operator `s`.
    pub(super) fn is(&self, i: usize, s: &str) -> bool {
        self.toks
            .get(i)
            .is_some_and(|tok| matches!(tok.t, T::Keyword | T::Op) && self.text(i) == s)
    }

    fn want(&self, i: usize, s: &str) -> Option<()> {
        self.is(i, s).then_some(())
    }

    fn end(&self, i: usize) -> usize {
        self.toks[i].end
    }

    /// The bracket closing the one at `i`.
    pub(super) fn close(&self, i: usize) -> Option<usize> {
        let mut depth = 0usize;
        for k in i..self.toks.len() {
            if self.toks[k].t != T::Op {
                continue;
            }
            match self.text(k) {
                "(" | "[" | "{" => depth += 1,
                ")" | "]" | "}" => {
                    depth = depth.checked_sub(1)?;
                    if depth == 0 {
                        return Some(k);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Statements from `i` up to one of `terms` (or the end of the text,
    /// when `terms` is empty); also the terminator's index.
    pub(super) fn block(&self, mut i: usize, terms: &[&str], lo: usize) -> Option<(Block, usize)> {
        let mut stmts = Vec::new();
        loop {
            let Some(tok) = self.toks.get(i) else {
                let block = Block {
                    stmts,
                    lo,
                    hi: self.src.len(),
                };
                return terms.is_empty().then_some((block, i));
            };
            if tok.t == T::Keyword && terms.contains(&self.text(i)) {
                let hi = tok.start;
                return Some((Block { stmts, lo, hi }, i));
            }
            if self.is(i, ";") {
                stmts.push(Stmt {
                    lo: i,
                    hi: i + 1,
                    shape: Shape::Other,
                });
                i += 1;
                continue;
            }
            let (mut stmt, mut next) = self.stmt(i)?;
            if self.is(next, ";") {
                next += 1;
                stmt.hi = next;
                stmt.shape = Shape::Other;
            }
            stmts.push(stmt);
            i = next;
        }
    }

    fn stmt(&self, i: usize) -> Option<(Stmt, usize)> {
        let done = |shape, hi| Some((Stmt { lo: i, hi, shape }, hi));
        if self.toks[i].t == T::Keyword {
            match self.text(i) {
                "if" => return self.if_stmt(i),
                "while" => {
                    let j = self.expr(i + 1)?;
                    self.want(j, "do")?;
                    let (_, k) = self.block(j + 1, &["end"], self.end(j))?;
                    return done(Shape::Other, k + 1);
                }
                "do" => {
                    let (_, k) = self.block(i + 1, &["end"], self.end(i))?;
                    return done(Shape::Other, k + 1);
                }
                "for" => return self.for_stmt(i),
                "repeat" => {
                    let (_, k) = self.block(i + 1, &["until"], self.end(i))?;
                    let j = self.expr(k + 1)?;
                    return (j > k + 1).then_some(()).and(done(Shape::Other, j));
                }
                "function" => return done(Shape::Other, self.func_body(i + 1)?),
                "local" if self.is(i + 1, "function") => {
                    return done(Shape::Other, self.func_body(i + 2)?);
                }
                "return" => {
                    let bare = i + 1 == self.toks.len()
                        || ["end", "else", "elseif", "until", ";"]
                            .iter()
                            .any(|s| self.is(i + 1, s));
                    let j = match bare {
                        true => i + 1,
                        false => self.expr(i + 1)?,
                    };
                    return done(Shape::Other, j);
                }
                "break" => return done(Shape::Other, i + 1),
                "local" => {
                    let j = self.expr(i + 1)?;
                    return (j > i + 1).then_some(()).and(done(Shape::Plain, j));
                }
                _ => {}
            }
        }
        let j = self.expr(i)?;
        (j > i).then_some(()).and(done(Shape::Plain, j))
    }

    fn if_stmt(&self, i: usize) -> Option<(Stmt, usize)> {
        let c = self.expr(i + 1)?;
        if c == i + 1 {
            return None;
        }
        self.want(c, "then")?;
        let (yes, mut k) = self.block(c + 1, IF_ENDS, self.end(c))?;
        let mut chained = false;
        let mut no = None;
        loop {
            match self.text(k) {
                "end" => break,
                "elseif" => {
                    chained = true;
                    let c = self.expr(k + 1)?;
                    self.want(c, "then")?;
                    k = self.block(c + 1, IF_ENDS, self.end(c))?.1;
                }
                "else" => {
                    let (block, end) = self.block(k + 1, &["end"], self.end(k))?;
                    no = Some(block);
                    k = end;
                    break;
                }
                _ => return None,
            }
        }
        let shape = match chained {
            true => Shape::Other,
            false => Shape::If {
                cond: (i + 1, c),
                yes,
                no,
            },
        };
        Some((
            Stmt {
                lo: i,
                hi: k + 1,
                shape,
            },
            k + 1,
        ))
    }

    fn for_stmt(&self, i: usize) -> Option<(Stmt, usize)> {
        let mut k = i + 1;
        while k < self.toks.len() && !self.is(k, "in") && !self.is(k, "=") {
            if self.toks[k].t == T::Keyword {
                return None;
            }
            k += 1;
        }
        let j = self.expr(k + 1)?;
        if j == k + 1 {
            return None;
        }
        self.want(j, "do")?;
        let (body, end) = self.block(j + 1, &["end"], self.end(j))?;
        let shape = Shape::For {
            head: (i + 1, j),
            body,
        };
        Some((
            Stmt {
                lo: i,
                hi: end + 1,
                shape,
            },
            end + 1,
        ))
    }

    /// Past the `end` of a function whose parameter list is the first `(`
    /// from `k`.
    fn func_body(&self, mut k: usize) -> Option<usize> {
        while k < self.toks.len() && !self.is(k, "(") {
            if self.toks[k].t == T::Keyword {
                return None;
            }
            k += 1;
        }
        let close = self.close(k)?;
        let (_, end) = self.block(close + 1, &["end"], self.end(close))?;
        Some(end + 1)
    }

    /// Where an expression (or an expression statement, or a `local`'s
    /// tail) starting at `i` ends: the first token that cannot continue
    /// it. `None` when its brackets do not balance.
    pub(super) fn expr(&self, i: usize) -> Option<usize> {
        let mut depth = 0usize;
        let mut ifs = 0usize;
        let mut ending = false;
        let mut k = i;
        while k < self.toks.len() {
            let text = self.text(k);
            match self.toks[k].t {
                T::Keyword => match text {
                    "function" => {
                        if depth == 0 && ending {
                            break;
                        }
                        k = self.func_body(k + 1)?;
                        ending = true;
                        continue;
                    }
                    "if" => {
                        if depth == 0 && ending {
                            break;
                        }
                        ifs += 1;
                        ending = false;
                    }
                    "then" | "else" | "elseif" if ifs > 0 => {
                        if text == "else" {
                            ifs -= 1;
                        }
                        ending = false;
                    }
                    "then" | "else" | "elseif" | "end" | "until" | "do" | "local" | "for"
                    | "while" | "repeat" | "return" | "break" | "in" => {
                        if depth == 0 {
                            break;
                        }
                        return None;
                    }
                    "nil" | "true" | "false" => {
                        if depth == 0 && ending {
                            break;
                        }
                        ending = true;
                    }
                    _ => ending = false,
                },
                T::Name => {
                    if depth == 0 && ending {
                        break;
                    }
                    ending = true;
                }
                T::Number | T::Str | T::Interp => ending = true,
                T::Op => match text {
                    "(" | "[" | "{" => {
                        depth += 1;
                        ending = false;
                    }
                    ")" | "]" | "}" => {
                        if depth == 0 {
                            break;
                        }
                        depth -= 1;
                        ending = true;
                    }
                    ";" => {
                        if depth == 0 {
                            break;
                        }
                        return None;
                    }
                    _ => ending = false,
                },
            }
            k += 1;
        }
        (depth == 0).then_some(k)
    }
}
