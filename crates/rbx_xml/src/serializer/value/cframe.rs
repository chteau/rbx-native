//! Encoders for CFrame-shaped types: CoordinateFrame and its Optional wrapper.
//! Mirrors `value::cframe`.

use rbx_dom::CFrameData;

use crate::serializer::writer::Writer;

// Row-major, same field order xml.md documents: R00, R01, R02, R10, ...
fn fields(writer: &mut Writer, cframe: &CFrameData) {
    writer.leaf("X", &[], &cframe.position.x.to_string());
    writer.leaf("Y", &[], &cframe.position.y.to_string());
    writer.leaf("Z", &[], &cframe.position.z.to_string());
    const LABELS: [&str; 9] = [
        "R00", "R01", "R02", "R10", "R11", "R12", "R20", "R21", "R22",
    ];
    for (label, value) in LABELS.iter().zip(cframe.rotation) {
        writer.leaf(label, &[], &value.to_string());
    }
}

pub(crate) fn cframe(writer: &mut Writer, name: &str, cframe_data: &CFrameData) {
    writer.open("CoordinateFrame", &[("name", name)]);
    fields(writer, cframe_data);
    writer.close("CoordinateFrame");
}

// The nested child is `CFrame`, not `CoordinateFrame`, per xml.md's own worked
// example (and the reader's `optional_cframe`, which looks for that exact tag).
pub(crate) fn optional_cframe(writer: &mut Writer, name: &str, value: &Option<CFrameData>) {
    writer.open("OptionalCoordinateFrame", &[("name", name)]);
    if let Some(cframe_data) = value {
        writer.open("CFrame", &[]);
        fields(writer, cframe_data);
        writer.close("CFrame");
    }
    writer.close("OptionalCoordinateFrame");
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_dom::Vector3Data;

    fn identity() -> CFrameData {
        CFrameData {
            position: Vector3Data {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        }
    }

    #[test]
    fn optional_cframe_none_has_no_nested_cframe() {
        let mut writer = Writer::new();
        optional_cframe(&mut writer, "X", &None);
        assert_eq!(
            writer.into_string(),
            "<OptionalCoordinateFrame name=\"X\">\n</OptionalCoordinateFrame>\n"
        );
    }

    #[test]
    fn optional_cframe_some_wraps_a_cframe_child() {
        let mut writer = Writer::new();
        optional_cframe(&mut writer, "X", &Some(identity()));
        assert!(writer.into_string().contains("<CFrame>\n"));
    }
}
