//! The `StyleRule.Selector` grammar, parsed into something [`super::matcher`]
//! can walk.
//!
//! Roblox's own summary of it (`StyleRule.Selector`, creator-docs): a class
//! name matches a class, `.Tag` a `CollectionService` tag, `#Name` an
//! `Instance.Name`, `:State` a `GuiState`, `@Query` an active `StyleQuery`,
//! `::UIComponent` creates a phantom modifier instance, `>` is the child
//! combinator, `>>` the descendant one (not whitespace, unlike CSS), and `,`
//! separates independent selectors.

/// What joins one compound selector to the one before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Combinator {
    /// `>`: the previous match is the direct parent.
    Child,
    /// `>>`: the previous match is any ancestor.
    Descendant,
}

/// The conditions one instance has to meet, all of them at once.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Compound {
    /// A bare class name. The docs do not say whether a class selector also
    /// matches subclasses, so this matches the class exactly, as CSS element
    /// selectors do — a `Frame` rule then cannot leak onto a `ScrollingFrame`.
    pub(super) class: Option<String>,
    /// `#Name`. Roblox does not require these to be unique (css-comparisons).
    pub(super) name: Option<String>,
    /// `.Tag`, every one of which has to be present.
    pub(super) tags: Vec<String>,
}

/// One selector out of a rule's comma-separated list, rightmost compound
/// first: matching starts at the instance and walks up its ancestors.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Selector {
    pub(super) subject: Compound,
    /// Each ancestor step, nearest first, with the combinator that reached it.
    pub(super) ancestors: Vec<(Combinator, Compound)>,
    /// Set by anything this viewer can never satisfy: a `:State` selector (it
    /// draws one static frame, so no instance is ever hovered, pressed or
    /// selected), an `@Query` (no `StyleQuery` instances, and the built-in
    /// queries describe an input device and a viewport this renderer has no
    /// notion of), or a `::UIComponent` modifier (a phantom instance the plan
    /// has nowhere to put). The selector parses, and then never matches.
    pub(super) inactive: bool,
}

/// A rule's whole `Selector` string: any of these matching styles the
/// instance. An unparseable selector yields no entries at all, which is
/// Roblox reporting it through `StyleRule.SelectorError` and styling nothing.
pub(super) fn parse(selector: &str) -> Vec<Selector> {
    selector
        .split(',')
        .map(parse_one)
        .collect::<Option<Vec<_>>>()
        .unwrap_or_default()
}

fn parse_one(selector: &str) -> Option<Selector> {
    let mut steps: Vec<(Combinator, Compound)> = Vec::new();
    let mut current = Compound::default();
    let mut started = false;
    let mut inactive = false;
    // A gap only separates a compound from a combinator: `Frame > .Tag` is two
    // compounds, `Frame .Tag` is not a selector at all.
    let mut gap = false;

    let mut chars = selector.chars().peekable();
    while let Some(&character) = chars.peek() {
        if character.is_whitespace() {
            chars.next();
            gap = started;
            continue;
        }
        if character == '>' {
            chars.next();
            let combinator = if chars.peek() == Some(&'>') {
                chars.next();
                Combinator::Descendant
            } else {
                Combinator::Child
            };
            // A leading combinator is how a nested rule says "a child of my
            // parent rule's match"; `super::cascade` has already merged those
            // away, so one here has nothing on its left.
            if !started {
                return None;
            }
            steps.push((combinator, std::mem::take(&mut current)));
            started = false;
            gap = false;
            continue;
        }
        if gap {
            return None;
        }
        match character {
            ':' => {
                chars.next();
                // `::` is a modifier, and everything after it — including a
                // `#Alias` past a space — belongs to the phantom instance.
                if chars.peek() == Some(&':') {
                    inactive = true;
                    break;
                }
                word(&mut chars)?;
                inactive = true;
            }
            '@' => {
                chars.next();
                word(&mut chars)?;
                inactive = true;
            }
            '.' => {
                chars.next();
                current.tags.push(word(&mut chars)?);
            }
            '#' => {
                chars.next();
                current.name = Some(word(&mut chars)?);
            }
            _ => {
                // A second bare class name in one compound is the CSS
                // whitespace descendant syntax, which Roblox spells `>>`.
                if current.class.is_some() {
                    return None;
                }
                current.class = Some(word(&mut chars)?);
            }
        }
        started = true;
    }

    if !started {
        // A trailing combinator, or nothing at all. `::UICorner` on its own is
        // the exception: it is a whole selector, and an inactive one.
        return (inactive && steps.is_empty()).then(|| Selector {
            subject: Compound::default(),
            ancestors: Vec::new(),
            inactive,
        });
    }

    // Parsed left to right, each compound holding the combinator that leads
    // from it to the next; reversed, that is each ancestor step nearest first.
    Some(Selector {
        subject: current,
        ancestors: steps.into_iter().rev().collect(),
        inactive,
    })
}

/// One identifier: class, tag, name, state or query.
///
/// Roblox allows any string as an `Instance.Name`, and the docs do not say how
/// a `#Name` selector spells one that needs escaping, so this takes the
/// conservative set — an empty identifier is a syntax error.
fn word(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<String> {
    let mut word = String::new();
    while let Some(&character) = chars.peek() {
        if character.is_alphanumeric() || character == '_' {
            word.push(character);
            chars.next();
        } else {
            break;
        }
    }
    (!word.is_empty()).then_some(word)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn only(selector: &str) -> Selector {
        let mut parsed = parse(selector);
        assert_eq!(parsed.len(), 1, "parsing {selector:?}");
        parsed.remove(0)
    }

    fn compound(class: Option<&str>, name: Option<&str>, tags: &[&str]) -> Compound {
        Compound {
            class: class.map(str::to_owned),
            name: name.map(str::to_owned),
            tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
        }
    }

    #[test]
    fn a_compound_collects_class_name_and_tags() {
        let parsed = only("ImageLabel#Icon.Big.Round");

        assert_eq!(
            parsed.subject,
            compound(Some("ImageLabel"), Some("Icon"), &["Big", "Round"])
        );
        assert!(parsed.ancestors.is_empty());
        assert!(!parsed.inactive);
    }

    /// The example `StyleRule.Selector` itself gives.
    #[test]
    fn roblox_own_example_parses_right_to_left() {
        let parsed = only(".Container > ImageLabel.BlueOnHover:Hover");

        assert_eq!(
            parsed.subject,
            compound(Some("ImageLabel"), None, &["BlueOnHover"])
        );
        assert_eq!(
            parsed.ancestors,
            [(Combinator::Child, compound(None, None, &["Container"]))]
        );
        // `:Hover` is a state this viewer is never in.
        assert!(parsed.inactive);
    }

    #[test]
    fn a_double_angle_is_the_descendant_combinator() {
        let parsed = only("ImageButton >> .BlueOnHover");

        assert_eq!(parsed.subject, compound(None, None, &["BlueOnHover"]));
        assert_eq!(
            parsed.ancestors,
            [(
                Combinator::Descendant,
                compound(Some("ImageButton"), None, &[])
            )]
        );
    }

    #[test]
    fn a_chain_reverses_into_nearest_ancestor_first() {
        let parsed = only("Frame > ScrollingFrame >> TextLabel");

        assert_eq!(parsed.subject, compound(Some("TextLabel"), None, &[]));
        assert_eq!(
            parsed.ancestors,
            [
                (
                    Combinator::Descendant,
                    compound(Some("ScrollingFrame"), None, &[])
                ),
                (Combinator::Child, compound(Some("Frame"), None, &[])),
            ]
        );
    }

    #[test]
    fn a_comma_starts_an_independent_selector() {
        let parsed = parse("Frame.TagA, TextLabel.TagA");

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].subject, compound(Some("Frame"), None, &["TagA"]));
        assert_eq!(
            parsed[1].subject,
            compound(Some("TextLabel"), None, &["TagA"])
        );
    }

    #[test]
    fn a_query_or_a_modifier_parses_but_never_matches() {
        assert!(only("Frame@StyleQuerySmall").inactive);
        assert!(only("Frame.RoundedCorner20::UICorner").inactive);
        // A modifier is a whole selector on its own in a nested rule.
        assert!(only("::UIStroke #Outer").inactive);
    }

    #[test]
    fn a_modifier_keeps_the_compound_it_hangs_off() {
        let parsed = only("Frame::UICorner");

        assert_eq!(parsed.subject, compound(Some("Frame"), None, &[]));
    }

    #[test]
    fn a_selector_that_does_not_parse_styles_nothing() {
        // Whitespace is not the descendant combinator here, unlike CSS.
        assert!(parse("Frame TextLabel").is_empty());
        assert!(parse("Frame Frame").is_empty());
        assert!(parse("FrameTextLabel, Frame >").is_empty());
        assert!(parse("> Frame").is_empty());
        assert!(parse("Frame.").is_empty());
        assert!(parse("#").is_empty());
        assert!(parse("").is_empty());
        // Undocumented: CSS's universal selector has no Roblox equivalent.
        assert!(parse("*").is_empty());
    }
}
