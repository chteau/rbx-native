//! Text rendering of a DOM tree with property values.

use rbx_dom::{Axes, CFrameData, Content, Faces, Font, Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

const INDENT: &str = "  ";

/// Renders a DOM tree to a human-readable indented text format.
///
/// Each instance shows its class, name, and referent ID on one line,
/// followed by properties. Enum values are resolved to names where possible.
pub fn render_tree(dom: &WeakDom, db: &ReflectionDatabase) -> String {
    let mut out = String::new();

    for &referent in dom.root_refs() {
        render_instance(dom, db, referent, 0, &mut out);
    }

    out
}

fn render_instance(
    dom: &WeakDom,
    db: &ReflectionDatabase,
    referent: Ref,
    depth: usize,
    out: &mut String,
) {
    let Some(instance) = dom.get(referent) else {
        return;
    };

    out.push_str(&INDENT.repeat(depth));
    out.push_str(&format!(
        "{} \"{}\" ({})\n",
        instance.class(),
        instance.name(),
        referent.value()
    ));

    for (name, value) in instance.properties() {
        out.push_str(&INDENT.repeat(depth + 1));
        out.push_str(&format!(
            "{name} = {}\n",
            format_value(instance.class(), name, value, db)
        ));
    }

    for &child in instance.children() {
        render_instance(dom, db, child, depth + 1, out);
    }
}

fn format_value(class: &str, prop_name: &str, value: &Variant, db: &ReflectionDatabase) -> String {
    match value {
        Variant::String(s) => format!("{s:?}"),
        Variant::Bool(b) => b.to_string(),
        Variant::Int32(v) => v.to_string(),
        Variant::Int64(v) => v.to_string(),
        Variant::Float32(v) => v.to_string(),
        Variant::Float64(v) => v.to_string(),
        Variant::BrickColor(number) => format!("BrickColor({number})"),
        Variant::Color3(c) => format!("({}, {}, {})", c.r, c.g, c.b),
        Variant::Color3uint8 { r, g, b } => format!("({r}, {g}, {b})"),
        Variant::Vector2(v) => format!("({}, {})", v.x, v.y),
        Variant::Vector3(v) => format!("({}, {}, {})", v.x, v.y, v.z),
        Variant::Vector3int16 { x, y, z } => format!("({x}, {y}, {z})"),
        Variant::Ray { origin, direction } => format!(
            "Ray {{ origin: ({}, {}, {}), direction: ({}, {}, {}) }}",
            origin.x, origin.y, origin.z, direction.x, direction.y, direction.z
        ),
        Variant::Faces(faces) => format_faces(faces),
        Variant::Axes(axes) => format_axes(axes),
        Variant::CFrame(frame) => format_cframe(frame),
        Variant::OptionalCFrame(None) => "none".to_owned(),
        Variant::OptionalCFrame(Some(frame)) => format_cframe(frame),
        Variant::Enum(raw) => format_enum(class, prop_name, *raw, db),
        Variant::Ref(r) => format!("Ref({})", r.value()),
        Variant::NumberSequence(sequence) => format!(
            "NumberSequence[{}]",
            join(&sequence.keypoints, |k| format!(
                "{}: {} ±{}",
                k.time, k.value, k.envelope
            ))
        ),
        Variant::ColorSequence(sequence) => format!(
            "ColorSequence[{}]",
            join(&sequence.keypoints, |k| format!(
                "{}: ({}, {}, {})",
                k.time, k.color.r, k.color.g, k.color.b
            ))
        ),
        Variant::NumberRange(range) => format!("[{}, {}]", range.min, range.max),
        Variant::Rect(rect) => format!(
            "{{({}, {}), ({}, {})}}",
            rect.min.x, rect.min.y, rect.max.x, rect.max.y
        ),
        Variant::PhysicalProperties(p) => format!("{p:?}"),
        Variant::SharedString(id) => format!("SharedString({id})"),
        Variant::UDim(u) => format!("{{{}, {}}}", u.scale, u.offset),
        Variant::UDim2(u) => format!(
            "{{{{{}, {}}}, {{{}, {}}}}}",
            u.x.scale, u.x.offset, u.y.scale, u.y.offset
        ),
        // Concatenated in wire order so two ids from one save session line up
        // visually on their shared time and random halves.
        Variant::UniqueId(id) => {
            format!("{:08x}{:08x}{:016x}", id.index, id.time, id.random as u64)
        }
        Variant::Font(font) => format_font(font),
        // Hexadecimal because every bit is an independent capability flag.
        Variant::SecurityCapabilities(bits) => format!("SecurityCapabilities({bits:#x})"),
        Variant::Content(Content::None) => "Content(none)".to_owned(),
        Variant::Content(Content::Uri(uri)) => format!("Content({uri:?})"),
        Variant::Content(Content::Object(r)) => format!("Content(Ref({}))", r.value()),
        Variant::Unknown { type_id, raw } => {
            format!("Unknown(type={type_id:#04x}, len={})", raw.len())
        }
    }
}

fn format_cframe(frame: &CFrameData) -> String {
    format!(
        "pos=({}, {}, {}) rot={:?}",
        frame.position.x, frame.position.y, frame.position.z, frame.rotation
    )
}

// Named in wire bit order (Front, Bottom, Left, Back, Top, Right) rather than
// alphabetically, so the rendered name order matches the binary spec.
fn format_faces(faces: &Faces) -> String {
    let mut names = Vec::new();
    if faces.front {
        names.push("Front");
    }
    if faces.bottom {
        names.push("Bottom");
    }
    if faces.left {
        names.push("Left");
    }
    if faces.back {
        names.push("Back");
    }
    if faces.top {
        names.push("Top");
    }
    if faces.right {
        names.push("Right");
    }
    format!("Faces({})", names.join("|"))
}

fn format_axes(axes: &Axes) -> String {
    let mut names = Vec::new();
    if axes.x {
        names.push("X");
    }
    if axes.y {
        names.push("Y");
    }
    if axes.z {
        names.push("Z");
    }
    format!("Axes({})", names.join("|"))
}

fn format_font(font: &Font) -> String {
    let mut out = format!(
        "Font {{ family: {:?}, weight: {}, style: {:?}",
        font.family, font.weight, font.style
    );
    if let Some(cached) = &font.cached_face_id {
        out.push_str(&format!(", cached: {cached:?}"));
    }
    out.push_str(" }");
    out
}

fn join<T>(items: &[T], format_one: impl Fn(&T) -> String) -> String {
    items
        .iter()
        .map(format_one)
        .collect::<Vec<String>>()
        .join(", ")
}

// Enum values are only meaningful once resolved through the property's
// declared enum type; when the reflection dump doesn't know the property
// (or the ordinal is stale/custom), fall back to the raw number.
fn format_enum(class: &str, prop_name: &str, raw: u32, db: &ReflectionDatabase) -> String {
    let resolved = db
        .resolve_property(class, prop_name)
        .and_then(|prop| db.enum_name(&prop.value_type, raw));

    match resolved {
        Some(name) => format!("{raw} ({name})"),
        None => raw.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_dom::Instance;

    fn database() -> ReflectionDatabase {
        crate::reflection::default_reflection_database()
    }

    #[test]
    fn renders_single_root_instance_with_properties() {
        let mut dom = WeakDom::new();
        let referent = Ref::new(1);
        let mut instance = Instance::new(referent, "Part", "MyPart");
        instance
            .properties_mut()
            .insert("Material".to_string(), Variant::Enum(272));
        dom.insert(instance);

        let output = render_tree(&dom, &database());

        assert!(output.contains("Part \"MyPart\" (1)"));
        assert!(output.contains("Material = 272 (SmoothPlastic)"));
    }

    #[test]
    fn indentation_grows_with_depth() {
        let mut dom = WeakDom::new();
        let parent = Ref::new(1);
        let child = Ref::new(2);
        dom.insert(Instance::new(parent, "Workspace", "Workspace"));
        dom.insert(Instance::new(child, "Part", "Part"));
        dom.set_parent(child, Some(parent));

        let output = render_tree(&dom, &database());
        let parent_line = output.lines().find(|l| l.contains("Workspace")).unwrap();
        let child_line = output.lines().find(|l| l.contains("Part")).unwrap();

        let parent_indent = parent_line.len() - parent_line.trim_start().len();
        let child_indent = child_line.len() - child_line.trim_start().len();
        assert!(child_indent > parent_indent);
    }

    #[test]
    fn unresolvable_enum_falls_back_to_raw_ordinal() {
        let mut dom = WeakDom::new();
        let referent = Ref::new(1);
        let mut instance = Instance::new(referent, "Part", "MyPart");
        instance
            .properties_mut()
            .insert("TotallyMadeUpProperty".to_string(), Variant::Enum(9999));
        dom.insert(instance);

        let output = render_tree(&dom, &database());
        assert!(output.contains("TotallyMadeUpProperty = 9999\n"));
    }
}
