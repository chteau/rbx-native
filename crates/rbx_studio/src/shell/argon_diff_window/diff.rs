//! A line diff for the Diff window's code card: Myers' O(ND) shortest
//! edit script over lines, then the edits grouped into hunks with three
//! lines of context each side, the way `diff -u` (and `similar`'s
//! `grouped_ops(3)`) lays a unified diff out. Pure, so it is unit-tested
//! on the fixture pair the window ships with.

/// One line of the unified view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Op {
    /// The same line in both: `(old index, new index)`, zero-based.
    Equal(usize, usize),
    /// A line of the old source that is gone.
    Delete(usize),
    /// A line of the new source that is new.
    Insert(usize),
}

/// Every op in order, plus the counts the rows and the header show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LineDiff {
    pub(super) ops: Vec<Op>,
    pub(super) added: usize,
    pub(super) removed: usize,
}

/// Myers' algorithm, the classic forward-only form: `v[k]` holds the
/// furthest x reached on diagonal k for the current edit distance d,
/// and the trace of every d's `v` is walked back to recover the path.
pub(super) fn diff_lines(old: &[&str], new: &[&str]) -> LineDiff {
    let n = old.len();
    let m = new.len();
    let max = n + m;
    let offset = max;
    let mut v = vec![0usize; 2 * max + 2];
    let mut trace: Vec<Vec<usize>> = Vec::new();
    let mut found = None;
    'outer: for d in 0..=max {
        trace.push(v.clone());
        let mut k = -(d as isize);
        while k <= d as isize {
            let index = (k + offset as isize) as usize;
            let mut x = if k == -(d as isize) || (k != d as isize && v[index - 1] < v[index + 1]) {
                v[index + 1]
            } else {
                v[index - 1] + 1
            };
            let mut y = (x as isize - k) as usize;
            while x < n && y < m && old[x] == new[y] {
                x += 1;
                y += 1;
            }
            v[index] = x;
            if x >= n && y >= m {
                found = Some(d);
                break 'outer;
            }
            k += 2;
        }
    }
    let d = found.unwrap_or(0);

    // Walk the trace back from (n, m) to (0, 0), emitting ops in reverse.
    let mut ops = Vec::new();
    let (mut x, mut y) = (n, m);
    for depth in (0..=d).rev() {
        let v = &trace[depth];
        let k = x as isize - y as isize;
        let index = (k + offset as isize) as usize;
        // At depth 0 there is no move to undo: the snake runs back to the
        // origin. Above it, the move came from the neighbouring diagonal
        // that reached further.
        let (prev_x, prev_y) = if depth == 0 {
            (0, 0)
        } else {
            let prev_k =
                if k == -(depth as isize) || (k != depth as isize && v[index - 1] < v[index + 1]) {
                    k + 1
                } else {
                    k - 1
                };
            let prev_index = (prev_k + offset as isize) as usize;
            let prev_x = v[prev_index];
            (prev_x, (prev_x as isize - prev_k) as usize)
        };
        while x > prev_x && y > prev_y {
            x -= 1;
            y -= 1;
            ops.push(Op::Equal(x, y));
        }
        if depth > 0 {
            if x == prev_x {
                y -= 1;
                ops.push(Op::Insert(y));
            } else {
                x -= 1;
                ops.push(Op::Delete(x));
            }
        }
    }
    ops.reverse();
    let added = ops.iter().filter(|op| matches!(op, Op::Insert(_))).count();
    let removed = ops.iter().filter(|op| matches!(op, Op::Delete(_))).count();
    LineDiff {
        ops,
        added,
        removed,
    }
}

/// One hunk of the unified view: the ops it shows (context included), and
/// how many equal lines sit between the previous hunk (or the top) and it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Hunk {
    pub(super) ops: Vec<Op>,
    /// Equal ops skipped before this hunk's first shown line.
    pub(super) hidden_before: Vec<Op>,
    /// `@@ −a,b +c,d @@`: one-based starts and the counts.
    pub(super) old_start: usize,
    pub(super) old_count: usize,
    pub(super) new_start: usize,
    pub(super) new_count: usize,
}

/// The hunks, with `context` equal lines on each side of every change,
/// and the equal lines left after the last one.
pub(super) fn hunks(diff: &LineDiff, context: usize) -> (Vec<Hunk>, Vec<Op>) {
    let ops = &diff.ops;
    let changed: Vec<usize> = ops
        .iter()
        .enumerate()
        .filter(|(_, op)| !matches!(op, Op::Equal(..)))
        .map(|(index, _)| index)
        .collect();
    let mut hunks = Vec::new();
    let mut cursor = 0;
    let mut i = 0;
    while i < changed.len() {
        let start = changed[i].saturating_sub(context).max(cursor);
        let mut end = (changed[i] + context + 1).min(ops.len());
        let mut j = i + 1;
        while j < changed.len() && changed[j] <= end + context {
            end = (changed[j] + context + 1).min(ops.len());
            j += 1;
        }
        let shown = &ops[start..end];
        let (old_start, new_start) = first_positions(shown, ops, start);
        let old_count = shown
            .iter()
            .filter(|op| !matches!(op, Op::Insert(_)))
            .count();
        let new_count = shown
            .iter()
            .filter(|op| !matches!(op, Op::Delete(_)))
            .count();
        hunks.push(Hunk {
            ops: shown.to_vec(),
            hidden_before: ops[cursor..start].to_vec(),
            old_start,
            old_count,
            new_start,
            new_count,
        });
        cursor = end;
        i = j;
    }
    (hunks, ops[cursor..].to_vec())
}

/// The one-based old and new line numbers a run of ops starts at.
fn first_positions(shown: &[Op], all: &[Op], start: usize) -> (usize, usize) {
    let mut old = 0;
    let mut new = 0;
    for op in &all[..start] {
        match op {
            Op::Equal(..) => {
                old += 1;
                new += 1;
            }
            Op::Delete(_) => old += 1,
            Op::Insert(_) => new += 1,
        }
    }
    let _ = shown;
    (old + 1, new + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<&str> {
        text.lines().collect()
    }

    #[test]
    fn identical_sources_are_all_equal() {
        let diff = diff_lines(&lines("a\nb\nc"), &lines("a\nb\nc"));
        assert_eq!(
            diff.ops,
            vec![Op::Equal(0, 0), Op::Equal(1, 1), Op::Equal(2, 2)]
        );
        assert_eq!((diff.added, diff.removed), (0, 0));
        assert!(hunks(&diff, 3).0.is_empty());
    }

    #[test]
    fn an_insert_and_a_delete_land_where_they_happened() {
        let diff = diff_lines(&lines("a\nb\nc"), &lines("a\nx\nb"));
        assert_eq!((diff.added, diff.removed), (1, 1));
        assert_eq!(
            diff.ops,
            vec![
                Op::Equal(0, 0),
                Op::Insert(1),
                Op::Equal(1, 2),
                Op::Delete(2)
            ]
        );
    }

    #[test]
    fn hunks_carry_three_lines_of_context_and_the_rest_is_hidden() {
        let old: Vec<String> = (1..=20).map(|n| n.to_string()).collect();
        let mut new = old.clone();
        new[9] = "ten".into();
        let old: Vec<&str> = old.iter().map(String::as_str).collect();
        let new: Vec<&str> = new.iter().map(String::as_str).collect();
        let diff = diff_lines(&old, &new);
        let (hunks, tail) = hunks(&diff, 3);
        assert_eq!(hunks.len(), 1);
        let hunk = &hunks[0];
        assert_eq!(hunk.hidden_before.len(), 6);
        assert_eq!(
            (
                hunk.old_start,
                hunk.old_count,
                hunk.new_start,
                hunk.new_count
            ),
            (7, 7, 7, 7)
        );
        assert_eq!(hunk.ops.len(), 8);
        assert_eq!(tail.len(), 7);
    }

    #[test]
    fn the_fixture_pair_diffs_as_gnu_diff_does() {
        let old = include_str!("../../../../../assets/tests/argon_diff/RoundController.old.luau");
        let new = include_str!("../../../../../assets/tests/argon_diff/RoundController.new.luau");
        let diff = diff_lines(&lines(old), &lines(new));
        // The same +13 −10 the design quotes; the first hunk runs 3..16 in the
        // old source, its three lines of trailing context included.
        assert_eq!((diff.added, diff.removed), (13, 10));
        let (hunks, _) = hunks(&diff, 3);
        assert_eq!(hunks[0].hidden_before.len(), 2);
        assert_eq!(
            (
                hunks[0].old_start,
                hunks[0].old_count,
                hunks[0].new_start,
                hunks[0].new_count
            ),
            (3, 14, 3, 12)
        );
    }
}
