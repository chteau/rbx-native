//! What the Change Class picker lists: suggestions while nothing is typed,
//! a fuzzy search once something is.

use std::cmp::Reverse;

use rbx_reflection::ReflectionDatabase;

use super::is_target;
use crate::explorer::insert::Choice;

/// How many related classes are suggested. Enough for a part's whole family
/// of shapes and seats, few enough that the list stays a glance.
const RELATED: usize = 8;

/// The one class every other descends from. Everything is related there, so
/// the walk up for relatives stops short of it.
const INSTANCE: &str = "Instance";

/// The picker's rows, in two runs: `suggested` first (empty once anything is
/// typed), then `rest`.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Choices {
    pub(crate) suggested: Vec<Choice>,
    pub(crate) rest: Vec<Choice>,
}

impl Choices {
    pub(crate) fn iter(&self) -> impl Iterator<Item = &Choice> {
        self.suggested.iter().chain(&self.rest)
    }
}

/// Every class the picker lists for a selection of `sources` classes.
/// `recent` is what this session last changed things to, most recent first.
///
/// With no `query`, the relatives of the selection (see [`related`]) and then
/// `recent` are suggested ahead of every other class, which follow legal
/// first and alphabetically, the way the insert picker lists them. With one,
/// only the classes it matches are listed, best match first (see [`rank`]).
/// A class the selection cannot become is listed greyed rather than left
/// out, as it is in the insert picker.
pub(crate) fn choices(
    database: &ReflectionDatabase,
    sources: &[&str],
    recent: &[String],
    query: &str,
) -> Choices {
    let query = query.trim().to_lowercase();
    let legal = |class: &str| is_target(database, class) && !sources.iter().all(|s| *s == class);
    let choice = |class: &str| Choice {
        class: class.to_owned(),
        legal: legal(class),
    };
    let browsable = database
        .class_names()
        .filter(|class| database.is_browsable(class));

    if !query.is_empty() {
        let mut ranked: Vec<(Tier, Choice)> = browsable
            .filter_map(|class| rank(&query, class).map(|tier| (tier, choice(class))))
            .collect();
        ranked.sort_by(|(left_tier, left), (right_tier, right)| {
            (
                left_tier,
                Reverse(left.legal),
                left.class.len(),
                &left.class,
            )
                .cmp(&(
                    right_tier,
                    Reverse(right.legal),
                    right.class.len(),
                    &right.class,
                ))
        });
        return Choices {
            suggested: Vec::new(),
            rest: ranked.into_iter().map(|(_, choice)| choice).collect(),
        };
    }

    let mut suggested: Vec<String> = related(database, sources);
    for class in recent {
        if legal(class) && !suggested.contains(class) {
            suggested.push(class.clone());
        }
    }
    let mut rest: Vec<Choice> = browsable
        .filter(|class| !suggested.iter().any(|s| s == class))
        .map(choice)
        .collect();
    rest.sort_by(|left, right| {
        (Reverse(left.legal), left.class.to_lowercase())
            .cmp(&(Reverse(right.legal), right.class.to_lowercase()))
    });
    Choices {
        suggested: suggested.iter().map(|class| choice(class)).collect(),
        rest,
    }
}

/// `class` and every class above it, nearest first.
fn ancestry<'a>(database: &'a ReflectionDatabase, class: &str) -> impl Iterator<Item = &'a str> {
    let first = database
        .class(class)
        .map(|descriptor| descriptor.name.as_str());
    std::iter::successors(first, move |name| {
        database
            .class(name)
            .and_then(|descriptor| descriptor.superclass.as_deref())
            .filter(|superclass| database.class(superclass).is_some())
    })
}

/// The classes nearest `sources` in the class tree that they could become:
/// walk up from their nearest common superclass, one ancestor at a time,
/// taking whatever else descends from it — so a `Part` is offered `Seat`
/// and `SpawnLocation` (its own subclasses), then `WedgePart` (under
/// `FormFactorPart`), then `MeshPart` and `TrussPart` (under `BasePart`).
///
/// Within one ancestor, a class fewer creatable steps below it comes first:
/// a `UnionOperation` is a `PartOperation` before it is a part, and is a
/// worse guess for a plain block than a `TrussPart`. Abstract steps do not
/// count, so a `TextLabel` (under `GuiLabel`) is as near a `Frame` as a
/// `ScrollingFrame` is. Deprecated classes are never suggested.
fn related(database: &ReflectionDatabase, sources: &[&str]) -> Vec<String> {
    let Some(first) = sources.first() else {
        return Vec::new();
    };
    let up: Vec<&str> = ancestry(database, first)
        .filter(|ancestor| sources.iter().all(|s| database.is_subclass_of(s, ancestor)))
        .take_while(|ancestor| *ancestor != INSTANCE)
        .collect();
    let deprecated = |class: &str| database.class_tags(class).iter().any(|t| t == "Deprecated");

    let mut found: Vec<(usize, usize, &str)> = database
        .class_names()
        .filter(|class| is_target(database, class) && !deprecated(class))
        .filter(|class| !sources.iter().all(|s| s == class))
        .filter_map(|class| {
            let mut steps = 0;
            for ancestor in ancestry(database, class) {
                if let Some(distance) = up.iter().position(|u| *u == ancestor) {
                    return Some((distance, steps, class));
                }
                if database.is_creatable(ancestor) {
                    steps += 1;
                }
            }
            None
        })
        .collect();
    found.sort_unstable();
    found
        .into_iter()
        .take(RELATED)
        .map(|(_, _, class)| class.to_owned())
        .collect()
}

/// How good a match is, best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Tier {
    /// The name starts with the query: `spot` in `SpotLight`.
    Prefix,
    /// The query is the starts of the name's words, in order: `tl` in
    /// `TextLabel`, `meshp` in `MeshPart`.
    WordStart,
    /// The query's letters appear in the name, in order, anywhere.
    Scattered,
}

/// How `query` (already lower-case) matches `class`, or `None` when it does
/// not. Case never matters; the capitals only say where words begin.
pub(super) fn rank(query: &str, class: &str) -> Option<Tier> {
    let lower = class.to_lowercase();
    if lower.starts_with(query) {
        return Some(Tier::Prefix);
    }
    let words = words(class);
    let query: Vec<char> = query.chars().collect();
    if word_starts(&query, &words) {
        return Some(Tier::WordStart);
    }
    let mut letters = lower.chars();
    query
        .iter()
        .all(|wanted| letters.any(|letter| letter == *wanted))
        .then_some(Tier::Scattered)
}

/// `class` split where a new word begins — at a capital after a lower-case
/// letter or digit, and at the last capital of a run followed by lower case
/// (`UIListLayout` is `ui`, `list`, `layout`) — each lower-cased.
fn words(class: &str) -> Vec<Vec<char>> {
    let chars: Vec<char> = class.chars().collect();
    let mut words: Vec<Vec<char>> = Vec::new();
    for (index, &letter) in chars.iter().enumerate() {
        let before = index.checked_sub(1).map(|i| chars[i]);
        let after = chars.get(index + 1);
        let starts = match before {
            None => true,
            Some(before) if letter.is_uppercase() => {
                !before.is_uppercase() || after.is_some_and(|a| a.is_lowercase())
            }
            _ => false,
        };
        if starts {
            words.push(Vec::new());
        }
        if let Some(word) = words.last_mut() {
            word.extend(letter.to_lowercase());
        }
    }
    words
}

/// Whether `query` is a run of word prefixes, each from a later word than
/// the one before. Names are a handful of short words, so trying every split
/// costs nothing worth memoizing.
fn word_starts(query: &[char], words: &[Vec<char>]) -> bool {
    if query.is_empty() {
        return true;
    }
    words.iter().enumerate().any(|(index, word)| {
        (1..=word.len().min(query.len())).rev().any(|taken| {
            word[..taken] == query[..taken] && word_starts(&query[taken..], &words[index + 1..])
        })
    })
}
