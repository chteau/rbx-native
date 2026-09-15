//! Encoders for vector/geometric leaf types: Vector2, Vector3, Vector3int16,
//! Color3, Color3uint8, Rect2D, Ray. Mirrors `value::vector`.

use rbx_dom::{Color3Data, Rect, Vector2Data, Vector3Data};

use crate::serializer::writer::Writer;

fn xy(writer: &mut Writer, v: &Vector2Data) {
    writer.leaf("X", &[], &v.x.to_string());
    writer.leaf("Y", &[], &v.y.to_string());
}

fn xyz(writer: &mut Writer, v: &Vector3Data) {
    writer.leaf("X", &[], &v.x.to_string());
    writer.leaf("Y", &[], &v.y.to_string());
    writer.leaf("Z", &[], &v.z.to_string());
}

pub(crate) fn vector2(writer: &mut Writer, name: &str, v: &Vector2Data) {
    writer.open("Vector2", &[("name", name)]);
    xy(writer, v);
    writer.close("Vector2");
}

pub(crate) fn vector3(writer: &mut Writer, name: &str, v: &Vector3Data) {
    writer.open("Vector3", &[("name", name)]);
    xyz(writer, v);
    writer.close("Vector3");
}

pub(crate) fn vector3int16(writer: &mut Writer, name: &str, x: i16, y: i16, z: i16) {
    writer.open("Vector3int16", &[("name", name)]);
    writer.leaf("X", &[], &x.to_string());
    writer.leaf("Y", &[], &y.to_string());
    writer.leaf("Z", &[], &z.to_string());
    writer.close("Vector3int16");
}

pub(crate) fn color3(writer: &mut Writer, name: &str, c: &Color3Data) {
    writer.open("Color3", &[("name", name)]);
    writer.leaf("R", &[], &c.r.to_string());
    writer.leaf("G", &[], &c.g.to_string());
    writer.leaf("B", &[], &c.b.to_string());
    writer.close("Color3");
}

// Packs alpha as 0xFF, matching xml.md's note that Roblox's own writer always
// does (the reader ignores it on decode either way).
pub(crate) fn color3_uint8(writer: &mut Writer, name: &str, r: u8, g: u8, b: u8) {
    let packed = (0xFFu32 << 24) | (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
    writer.leaf("Color3uint8", &[("name", name)], &packed.to_string());
}

pub(crate) fn rect2d(writer: &mut Writer, name: &str, rect: &Rect) {
    writer.open("Rect2D", &[("name", name)]);
    writer.open("min", &[]);
    xy(writer, &rect.min);
    writer.close("min");
    writer.open("max", &[]);
    xy(writer, &rect.max);
    writer.close("max");
    writer.close("Rect2D");
}

pub(crate) fn ray(writer: &mut Writer, name: &str, origin: &Vector3Data, direction: &Vector3Data) {
    writer.open("Ray", &[("name", name)]);
    writer.open("origin", &[]);
    xyz(writer, origin);
    writer.close("origin");
    writer.open("direction", &[]);
    xyz(writer, direction);
    writer.close("direction");
    writer.close("Ray");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color3_uint8_packs_the_spec_example() {
        let mut writer = Writer::new();
        color3_uint8(&mut writer, "Color", 96, 64, 32);
        assert_eq!(
            writer.into_string(),
            "<Color3uint8 name=\"Color\">4284497952</Color3uint8>\n"
        );
    }
}
