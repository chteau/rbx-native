//! Line-based three-way text merge, for scripts' `Source`.

/// A run of merged text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextChunk {
    /// Lines both sides agree on (or that only one side changed).
    Clean(String),
    /// Lines both sides changed differently; each field holds that side's
    /// lines for the region, terminators included (possibly empty).
    Conflict {
        base: String,
        ours: String,
        theirs: String,
    },
}

/// Merges `ours` and `theirs`, both edited from `base`, line by line in the
/// diff3 manner: lines (each keeping its `\n` or `\r\n`; a last line may
/// lack one) are aligned base→ours and base→theirs by a longest common
/// subsequence; regions where only one side differs from base take that
/// side; regions both changed identically take the change; regions both
/// changed differently are a `Conflict`. Adjacent `Clean` chunks are joined,
/// so the result alternates; concatenating every `Clean` with each
/// conflict's `ours` gives back `ours` exactly when theirs changed nothing.
/// Two insertions at the same base position that differ are a conflict
/// with an empty `base`.
pub fn merge_text(base: &str, ours: &str, theirs: &str) -> Vec<TextChunk> {
    let b: Vec<&str> = base.split_inclusive('\n').collect();
    let o: Vec<&str> = ours.split_inclusive('\n').collect();
    let t: Vec<&str> = theirs.split_inclusive('\n').collect();
    let mo = align(&b, &o);
    let mt = align(&b, &t);

    let mut out = Vec::new();
    let (mut bs, mut os, mut ts) = (0, 0, 0);
    for i in 0..=b.len() {
        // The end of the text is a sync point every side shares.
        let (j, k) = if i == b.len() {
            (o.len(), t.len())
        } else {
            match (mo[i], mt[i]) {
                (Some(j), Some(k)) => (j, k),
                _ => continue,
            }
        };
        region(&mut out, &b[bs..i], &o[os..j], &t[ts..k]);
        if i < b.len() {
            push_clean(&mut out, b[i]);
        }
        (bs, os, ts) = (i + 1, j + 1, k + 1);
    }
    out
}

fn push_clean(out: &mut Vec<TextChunk>, text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(TextChunk::Clean(last)) = out.last_mut() {
        last.push_str(text);
    } else {
        out.push(TextChunk::Clean(text.to_owned()));
    }
}

/// Settles one region between two lines stable on both sides.
fn region(out: &mut Vec<TextChunk>, b: &[&str], o: &[&str], t: &[&str]) {
    if o == b {
        push_clean(out, &t.concat());
    } else if t == b || o == t {
        push_clean(out, &o.concat());
    } else {
        out.push(TextChunk::Conflict {
            base: b.concat(),
            ours: o.concat(),
            theirs: t.concat(),
        });
    }
}

/// For each line of `a`, the index of the line of `b` it is matched with in
/// a longest common subsequence.
fn align(a: &[&str], b: &[&str]) -> Vec<Option<usize>> {
    let mut map = vec![None; a.len()];
    // Trimming the common ends keeps the table small for the usual
    // few-lines edit of a large script.
    let mut lo = 0;
    while lo < a.len() && lo < b.len() && a[lo] == b[lo] {
        map[lo] = Some(lo);
        lo += 1;
    }
    let (mut ea, mut eb) = (a.len(), b.len());
    while ea > lo && eb > lo && a[ea - 1] == b[eb - 1] {
        ea -= 1;
        eb -= 1;
        map[ea] = Some(eb);
    }
    let (ma, mb) = (&a[lo..ea], &b[lo..eb]);
    let w = mb.len() + 1;
    let mut dp = vec![0u32; (ma.len() + 1) * w];
    for i in (0..ma.len()).rev() {
        for j in (0..mb.len()).rev() {
            dp[i * w + j] = if ma[i] == mb[j] {
                dp[(i + 1) * w + j + 1] + 1
            } else {
                dp[(i + 1) * w + j].max(dp[i * w + j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    while i < ma.len() && j < mb.len() {
        if ma[i] == mb[j] {
            map[lo + i] = Some(lo + j);
            i += 1;
            j += 1;
        } else if dp[(i + 1) * w + j] >= dp[i * w + j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    map
}
