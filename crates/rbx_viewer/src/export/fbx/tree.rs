//! FBX's node tree, built in memory and written as binary FBX 7.4 through
//! `fbxcel`'s writer, plus the `Properties70` entries every object carries.

use std::io::Cursor;

use fbxcel::low::v7400::ArrayAttributeEncoding;
use fbxcel::low::FbxVersion;
use fbxcel::writer::v7400::binary::{FbxFooter, Writer};

/// The creation time and file id the header names, and the footer id the
/// FBX SDK checks them against. The three only come as a set: these are
/// the fixed ones Blender's exporter writes, which the SDK and every reader
/// take, and they keep the output byte for byte the same between runs.
pub(super) const CREATION_TIME: &str = "1970-01-01 10:00:00:000";
pub(super) const FILE_ID: [u8; 16] = [
    0x28, 0xb3, 0x2a, 0xeb, 0xb6, 0x24, 0xcc, 0xc2, 0xbf, 0xc8, 0xb0, 0x2a, 0xa9, 0x2b, 0xfc, 0xf1,
];
const FOOTER_ID: [u8; 16] = [
    0xfa, 0xbc, 0xab, 0x09, 0xd0, 0xc8, 0xd4, 0x66, 0xb1, 0x76, 0xfb, 0x83, 0x1c, 0xf7, 0x26, 0x7e,
];

pub(super) enum Prop {
    Bool(bool),
    I32(i32),
    I64(i64),
    F64(f64),
    Str(String),
    Bytes(Vec<u8>),
    F64s(Vec<f64>),
    I32s(Vec<i32>),
}

impl From<i32> for Prop {
    fn from(value: i32) -> Self {
        Prop::I32(value)
    }
}

impl From<i64> for Prop {
    fn from(value: i64) -> Self {
        Prop::I64(value)
    }
}

impl From<f64> for Prop {
    fn from(value: f64) -> Self {
        Prop::F64(value)
    }
}

impl From<&str> for Prop {
    fn from(value: &str) -> Self {
        Prop::Str(value.to_owned())
    }
}

impl From<String> for Prop {
    fn from(value: String) -> Self {
        Prop::Str(value)
    }
}

pub(super) struct Node {
    name: &'static str,
    props: Vec<Prop>,
    children: Vec<Node>,
}

impl Node {
    pub(super) fn new(name: &'static str, props: Vec<Prop>) -> Self {
        Self {
            name,
            props,
            children: Vec::new(),
        }
    }

    /// A node holding one value, the commonest kind.
    pub(super) fn value(name: &'static str, value: impl Into<Prop>) -> Self {
        Self::new(name, vec![value.into()])
    }

    pub(super) fn with(mut self, child: Node) -> Self {
        self.children.push(child);
        self
    }

    pub(super) fn push(&mut self, child: Node) {
        self.children.push(child);
    }

    fn write<W: std::io::Write + std::io::Seek>(
        &self,
        writer: &mut Writer<W>,
    ) -> fbxcel::writer::v7400::binary::Result<()> {
        let mut attributes = writer.new_node(self.name)?;
        // Big arrays shrink a lot under zlib; a short one would grow.
        let encoding = |len: usize| {
            Some(if len > 16 {
                ArrayAttributeEncoding::Zlib
            } else {
                ArrayAttributeEncoding::Direct
            })
        };
        for prop in &self.props {
            match prop {
                Prop::Bool(value) => attributes.append_bool(*value)?,
                Prop::I32(value) => attributes.append_i32(*value)?,
                Prop::I64(value) => attributes.append_i64(*value)?,
                Prop::F64(value) => attributes.append_f64(*value)?,
                Prop::Str(value) => attributes.append_string_direct(value)?,
                Prop::Bytes(value) => attributes.append_binary_direct(value)?,
                Prop::F64s(values) => attributes
                    .append_arr_f64_from_iter(encoding(values.len()), values.iter().copied())?,
                Prop::I32s(values) => attributes
                    .append_arr_i32_from_iter(encoding(values.len()), values.iter().copied())?,
            }
        }
        for child in &self.children {
            child.write(writer)?;
        }
        writer.close_node()
    }
}

/// The whole file: header, `nodes` at the top level, footer.
pub(super) fn file(nodes: &[Node]) -> Result<Vec<u8>, String> {
    let fail =
        |err: fbxcel::writer::v7400::binary::Error| format!("could not write the FBX file: {err}");
    let mut writer = Writer::new(Cursor::new(Vec::new()), FbxVersion::V7_4).map_err(fail)?;
    for node in nodes {
        node.write(&mut writer).map_err(fail)?;
    }
    let footer = FbxFooter {
        unknown1: Some(&FOOTER_ID),
        ..FbxFooter::default()
    };
    Ok(writer
        .finalize_and_flush(&footer)
        .map_err(fail)?
        .into_inner())
}

/// `Name\0\x01Class`, how FBX names an object and says what it is.
pub(super) fn name_class(name: &str, class: &str) -> String {
    format!("{name}\u{0}\u{1}{class}")
}

/// One `P` entry of a `Properties70`: name, type, label, flags, values.
fn p(name: &str, kind: &str, label: &str, flags: &str, values: Vec<Prop>) -> Node {
    let mut props = vec![name.into(), kind.into(), label.into(), flags.into()];
    props.extend(values);
    Node::new("P", props)
}

pub(super) fn p_int(name: &str, value: i32) -> Node {
    p(name, "int", "Integer", "", vec![value.into()])
}

pub(super) fn p_enum(name: &str, value: i32) -> Node {
    p(name, "enum", "", "", vec![value.into()])
}

pub(super) fn p_double(name: &str, value: f64) -> Node {
    p(name, "double", "Number", "", vec![value.into()])
}

/// An animatable number, as a material's factors are.
pub(super) fn p_number(name: &str, value: f64) -> Node {
    p(name, "Number", "", "A", vec![value.into()])
}

pub(super) fn p_color(name: &str, rgb: [f64; 3]) -> Node {
    p(name, "Color", "", "A", rgb.map(Prop::F64).into())
}

/// `Lcl Translation`, `Lcl Rotation` and `Lcl Scaling`, typed by their name.
pub(super) fn p_local(name: &str, xyz: [f64; 3]) -> Node {
    p(name, name, "", "A", xyz.map(Prop::F64).into())
}

pub(super) fn p_string(name: &str, value: &str) -> Node {
    p(name, "KString", "", "", vec![value.into()])
}

/// A user property: what a DCC lists as custom data on the object.
pub(super) fn p_user(name: &str, value: &str) -> Node {
    p(name, "KString", "", "U", vec![value.into()])
}

pub(super) fn p_url(name: &str, value: &str) -> Node {
    p(name, "KString", "XRefUrl", "", vec![value.into()])
}

pub(super) fn properties(entries: Vec<Node>) -> Node {
    let mut node = Node::new("Properties70", Vec::new());
    node.children = entries;
    node
}
