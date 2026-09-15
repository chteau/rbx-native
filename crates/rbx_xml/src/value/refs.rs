//! Decoders for referent-shaped leaf types: Ref, SharedString, NetAssetRef.

use rbx_dom::Variant;

use super::scalar::string_value;
use super::{Ctx, UNKNOWN_TAG_TYPE_ID};

pub(crate) fn reference(text: &str, ctx: &Ctx<'_>) -> Option<Variant> {
    if text == "null" {
        // A null Ref is the DOM's representation of "no value" for this property,
        // the same convention the binary parser uses for its own null referent (-1).
        return None;
    }
    ctx.referents.get(text).map(|&r| Variant::Ref(r))
}

// SharedString and NetAssetRef are wire-identical: both point at the same table,
// keyed by its `md5` attribute.
pub(crate) fn shared_string(text: &str, ctx: &Ctx<'_>) -> Variant {
    match ctx.shared.get(text) {
        Some(bytes) => string_value(bytes),
        // No numeric SSTR index exists in XML to fall back to (unlike the binary
        // format's `Variant::SharedString(u32)`), so an unresolved key degrades to
        // `Unknown` instead, keeping the raw key visible rather than dropping it.
        None => Variant::Unknown {
            type_id: UNKNOWN_TAG_TYPE_ID,
            raw: text.as_bytes().to_vec(),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use rbx_dom::Ref;

    use super::*;

    #[test]
    fn null_ref_is_absent() {
        let referents = HashMap::new();
        let shared = HashMap::new();
        let ctx = Ctx {
            referents: &referents,
            shared: &shared,
        };
        assert_eq!(reference("null", &ctx), None);
    }

    #[test]
    fn ref_resolves_against_the_referent_table() {
        let mut referents = HashMap::new();
        referents.insert("RBXTarget".to_owned(), Ref::new(3));
        let shared = HashMap::new();
        let ctx = Ctx {
            referents: &referents,
            shared: &shared,
        };
        assert_eq!(
            reference("RBXTarget", &ctx),
            Some(Variant::Ref(Ref::new(3)))
        );
    }

    #[test]
    fn dangling_ref_is_absent() {
        let referents = HashMap::new();
        let shared = HashMap::new();
        let ctx = Ctx {
            referents: &referents,
            shared: &shared,
        };
        assert_eq!(reference("RBXMissing", &ctx), None);
    }

    #[test]
    fn shared_string_resolves_against_the_table() {
        let referents = HashMap::new();
        let mut shared = HashMap::new();
        shared.insert("md5key".to_owned(), b"tagged".to_vec());
        let ctx = Ctx {
            referents: &referents,
            shared: &shared,
        };
        assert_eq!(
            shared_string("md5key", &ctx),
            Variant::String("tagged".to_owned())
        );
    }
}
