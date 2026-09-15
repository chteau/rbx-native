//! PRNT chunk writing, the encode counterpart of `chunks::prnt`.

use crate::codec::encode_referents;

use super::plan::Plan;
use super::writer::Writer;

const NO_PARENT: i32 = -1;

/// Writes the PRNT chunk payload covering every instance in `plan`.
pub(crate) fn write(plan: &Plan) -> Vec<u8> {
    let mut writer = Writer::new();

    writer.u8(0); // version
    writer.length(plan.order.len());

    let children: Vec<i32> = plan.order.iter().map(|r| r.value() as i32).collect();
    let parents: Vec<i32> = plan
        .order
        .iter()
        .map(|referent| {
            plan.parents
                .get(referent)
                .map_or(NO_PARENT, |parent| parent.value() as i32)
        })
        .collect();

    writer.bytes(&encode_referents(&children));
    writer.bytes(&encode_referents(&parents));

    writer.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunks::prnt;
    use crate::serialize::plan;
    use rbx_dom::WeakDom;

    #[test]
    fn writes_links_the_reader_accepts_with_roots_at_minus_one() {
        let mut dom = WeakDom::new();
        let root = dom.new_instance("Folder", "Root", None);
        let child = dom.new_instance("Part", "Child", Some(root));

        let plan = plan::build(&dom);
        let links = prnt::parse(&write(&plan)).unwrap();

        assert_eq!(links.len(), 2);
        let by_child = |target: rbx_dom::Ref| {
            links
                .iter()
                .find(|link| link.child == target.value() as i32)
                .unwrap()
        };
        assert_eq!(by_child(root).parent, prnt::NO_PARENT);
        assert_eq!(by_child(child).parent, root.value() as i32);
    }
}
