//! INST chunk writing, the encode counterpart of `chunks::inst`.

use crate::codec::encode_referents;

use super::plan::ClassPlan;
use super::writer::Writer;

/// Writes one class's INST chunk payload.
pub(crate) fn write(class: &ClassPlan) -> Vec<u8> {
    let mut writer = Writer::new();

    writer.i32(class.class_id);
    writer.sized_name(&class.class_name);
    writer.u8(class.is_service as u8);
    writer.length(class.referents.len());

    let raw: Vec<i32> = class.referents.iter().map(|r| r.value() as i32).collect();
    writer.bytes(&encode_referents(&raw));

    if class.is_service {
        // The reader only skips this array; any byte value keeps it aligned. `1`
        // matches what real files write for the services in our fixtures.
        writer.bytes(&vec![1u8; class.referents.len()]);
    }

    writer.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunks::inst;
    use rbx_dom::Ref;

    #[test]
    fn writes_a_non_service_class_the_reader_accepts() {
        let class = ClassPlan {
            class_id: 3,
            class_name: "Part".to_owned(),
            is_service: false,
            referents: vec![Ref::new(5), Ref::new(6)],
        };

        let parsed = inst::parse(&write(&class)).unwrap();

        assert_eq!(parsed.class_id, 3);
        assert_eq!(parsed.class_name, "Part");
        assert_eq!(parsed.referents, vec![5, 6]);
    }

    #[test]
    fn writes_a_service_class_the_reader_accepts() {
        let class = ClassPlan {
            class_id: 0,
            class_name: "Workspace".to_owned(),
            is_service: true,
            referents: vec![Ref::new(1)],
        };

        let parsed = inst::parse(&write(&class)).unwrap();

        assert_eq!(parsed.referents, vec![1]);
    }
}
