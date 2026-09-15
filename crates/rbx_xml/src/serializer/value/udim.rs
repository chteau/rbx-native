//! Encoders for UDim and UDim2. Mirrors `value::udim`.

use rbx_dom::{UDim, UDim2};

use crate::serializer::writer::Writer;

fn write_udim(writer: &mut Writer, scale_tag: &str, offset_tag: &str, udim: &UDim) {
    writer.leaf(scale_tag, &[], &udim.scale.to_string());
    writer.leaf(offset_tag, &[], &udim.offset.to_string());
}

pub(crate) fn udim(writer: &mut Writer, name: &str, value: &UDim) {
    writer.open("UDim", &[("name", name)]);
    write_udim(writer, "S", "O", value);
    writer.close("UDim");
}

pub(crate) fn udim2(writer: &mut Writer, name: &str, value: &UDim2) {
    writer.open("UDim2", &[("name", name)]);
    write_udim(writer, "XS", "XO", &value.x);
    write_udim(writer, "YS", "YO", &value.y);
    writer.close("UDim2");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn udim2_writes_both_components() {
        let mut writer = Writer::new();
        udim2(
            &mut writer,
            "Size",
            &UDim2 {
                x: UDim {
                    scale: 0.5,
                    offset: 10,
                },
                y: UDim {
                    scale: -1.0,
                    offset: -5,
                },
            },
        );
        let out = writer.into_string();
        assert!(out.contains("<XS>0.5</XS>"));
        assert!(out.contains("<YO>-5</YO>"));
    }
}
