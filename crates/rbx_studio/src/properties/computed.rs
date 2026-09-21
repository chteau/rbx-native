//! The read-only values Studio's panel lists for a part that no file stores,
//! because a running engine works them out — here, only where Roblox's docs
//! say exactly how:
//!
//! - `Mass` is volume times density (`BasePart.Mass`). Volume is exact for a
//!   block, a wedge (half the block) and a corner wedge (a third), and for a
//!   ball whose three sides agree. Not for a cylinder, whose collision
//!   geometry `Part.Shape` calls an approximation, nor for a mesh, a union
//!   or a truss, whose volume is not the box's.
//! - Density, and the rest of `CurrentPhysicalProperties`, come from the
//!   first of the part's own `CustomPhysicalProperties`, its material
//!   variant's, its material override's, and the material's defaults
//!   (`BasePart.CurrentPhysicalProperties`, `parts/materials.md`).
//! - `CenterOfMass` is the shape's centroid: the origin for a block or a
//!   ball, as `BasePart.CenterOfMass` says, and off it for the wedges.
//! - `ResizeIncrement` and `ResizeableFaces` for the classes their docs
//!   name: 1 and every face for a `Part`, every face for a `WedgePart`.
//! - The assembly's mass, centre and root: see `assembly`.
//!
//! Left out, for want of anything true to show: the assembly's velocities
//! (a simulation's state), `ExtentsSize`/`ExtentsCFrame` (the physics
//! engine's own bounds, undocumented) and `Rotation` (its Euler order is
//! undocumented).

use std::f64::consts::PI;

use rbx_dom::{Faces, Instance, PhysicalProperties, Ref, Variant, Vector3Data, WeakDom};

use super::Properties;

mod assembly;
mod materials;

pub(super) use assembly::Joints;

/// What `Properties::computed` can answer for.
pub(super) const COMPUTED: &[&str] = &[
    "Mass",
    "CenterOfMass",
    "CurrentPhysicalProperties",
    "ResizeIncrement",
    "ResizeableFaces",
    "AssemblyMass",
    "AssemblyCenterOfMass",
    "AssemblyRootPart",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    Block,
    Ball,
    Wedge,
    CornerWedge,
}

impl Properties {
    /// `name` on `instance`, where it is one of [`COMPUTED`] and can be
    /// worked out; `None` otherwise.
    pub(super) fn computed(
        &self,
        dom: &WeakDom,
        reference: Ref,
        instance: &Instance,
        name: &str,
    ) -> Option<Variant> {
        match name {
            "Mass" => self.mass(dom, instance).map(Variant::Float32),
            "CenterOfMass" => self.center_of_mass(instance).map(Variant::Vector3),
            "CurrentPhysicalProperties" => {
                let [density, friction, elasticity, friction_weight, elasticity_weight] =
                    self.physics(dom, instance)?;
                Some(Variant::PhysicalProperties(PhysicalProperties::Custom {
                    density,
                    friction,
                    elasticity,
                    friction_weight,
                    elasticity_weight,
                }))
            }
            "ResizeIncrement" => match instance.class() {
                "Part" => Some(Variant::Int32(1)),
                "TrussPart" => Some(Variant::Int32(2)),
                _ => None,
            },
            // A truss's two faces are not named by its docs.
            "ResizeableFaces" => {
                matches!(instance.class(), "Part" | "WedgePart").then_some(Variant::Faces(Faces {
                    front: true,
                    bottom: true,
                    left: true,
                    back: true,
                    top: true,
                    right: true,
                }))
            }
            "AssemblyMass" | "AssemblyCenterOfMass" | "AssemblyRootPart" => {
                self.assembly(dom, reference, name)
            }
            _ => None,
        }
    }

    fn shape(&self, instance: &Instance) -> Option<Shape> {
        match instance.class() {
            "WedgePart" => Some(Shape::Wedge),
            "CornerWedgePart" => Some(Shape::CornerWedge),
            class if self.db.is_subclass_of(class, "Part") => {
                match self.read(instance, "Shape")? {
                    Variant::Enum(0) => Some(Shape::Ball),
                    Variant::Enum(1) => Some(Shape::Block),
                    Variant::Enum(3) => Some(Shape::Wedge),
                    Variant::Enum(4) => Some(Shape::CornerWedge),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn size(&self, instance: &Instance) -> Option<Vector3Data> {
        match self.read(instance, "Size")? {
            Variant::Vector3(size) => Some(size),
            _ => None,
        }
    }

    pub(super) fn mass(&self, dom: &WeakDom, instance: &Instance) -> Option<f32> {
        let shape = self.shape(instance)?;
        let size = self.size(instance)?;
        let [x, y, z] = [size.x, size.y, size.z].map(f64::from);
        let volume = match shape {
            Shape::Block => x * y * z,
            Shape::Wedge => x * y * z / 2.0,
            Shape::CornerWedge => x * y * z / 3.0,
            Shape::Ball if x == y && y == z => PI * x * x * x / 6.0,
            Shape::Ball => return None,
        };
        let [density, ..] = self.physics(dom, instance)?;
        Some(significant(volume * f64::from(density)))
    }

    /// The part's centre of mass in its own space.
    pub(super) fn center_of_mass(&self, instance: &Instance) -> Option<Vector3Data> {
        let size = self.size(instance)?;
        let (x, y, z) = match self.shape(instance)? {
            Shape::Block | Shape::Ball => (0.0, 0.0, 0.0),
            // The centroid of the wedge's cross-section: its vertical face
            // is at +Z, sloping down to the bottom edge at -Z (see
            // `rbx_viewer`'s `shapes::wedge`).
            Shape::Wedge => (0.0, -size.y / 6.0, size.z / 6.0),
            // A pyramid's centroid, a quarter of the way from its base's
            // centre to the apex above the (+X, -Z) corner.
            Shape::CornerWedge => (size.x / 8.0, -size.y / 4.0, -size.z / 8.0),
        };
        Some(Vector3Data { x, y, z })
    }

    /// Density, friction, elasticity and their two weights, in
    /// `PhysicalProperties.new`'s order: the first of the four sources the
    /// module doc lists that has its own.
    fn physics(&self, dom: &WeakDom, instance: &Instance) -> Option<[f32; 5]> {
        if let Some(custom) = custom(self.read(instance, "CustomPhysicalProperties")) {
            return Some(custom);
        }
        let material = match self.read(instance, "Material")? {
            Variant::Enum(raw) => self.db.enum_name("Material", raw)?,
            _ => return None,
        };
        let own_variant = match self.read(instance, "MaterialVariant") {
            Some(Variant::String(name)) => name,
            _ => String::new(),
        };
        if !own_variant.is_empty() {
            if let Some(custom) = self.variant_physics(dom, &own_variant, material)? {
                return Some(custom);
            }
        } else if let Some(name) = self.material_override(dom, material) {
            if let Some(custom) = self.variant_physics(dom, &name, material)? {
                return Some(custom);
            }
        }
        materials::MATERIALS
            .iter()
            .find(|(name, _)| *name == material)
            .map(|(_, physics)| *physics)
    }

    /// The name `MaterialService` overrides `material` with, when it names
    /// something other than the material itself.
    fn material_override(&self, dom: &WeakDom, material: &str) -> Option<String> {
        let service = material_service(dom)?;
        let property = format!("{material}Name");
        match self.read(service, &property)? {
            Variant::String(name) if name != material => Some(name),
            _ => None,
        }
    }

    /// The `CustomPhysicalProperties` of the `MaterialVariant` called `name`
    /// for `material`: `Some(None)` when it has none of its own, `None` when
    /// there is no such variant to read — which leaves the part's physics
    /// unknown rather than guessed.
    fn variant_physics(
        &self,
        dom: &WeakDom,
        name: &str,
        material: &str,
    ) -> Option<Option<[f32; 5]>> {
        let service = material_service(dom)?;
        let variant = service.children().iter().find_map(|&child| {
            let variant = dom.get(child)?;
            let base = match self.read(variant, "BaseMaterial")? {
                Variant::Enum(raw) => self.db.enum_name("Material", raw)?,
                _ => return None,
            };
            (variant.class() == "MaterialVariant" && variant.name() == name && base == material)
                .then_some(variant)
        })?;
        Some(custom(self.read(variant, "CustomPhysicalProperties")))
    }
}

/// `value` to the six significant digits an `f32` holds. A size of `1.2`
/// is stored as `1.2000000477`, and without this a new part's mass would
/// read `6.7200003` where the arithmetic it stands for is `6.72`.
fn significant(value: f64) -> f32 {
    if value == 0.0 || !value.is_finite() {
        return value as f32;
    }
    let scale = 10f64.powi(5 - value.abs().log10().floor() as i32);
    ((value * scale).round() / scale) as f32
}

fn custom(value: Option<Variant>) -> Option<[f32; 5]> {
    match value? {
        Variant::PhysicalProperties(PhysicalProperties::Custom {
            density,
            friction,
            elasticity,
            friction_weight,
            elasticity_weight,
        }) => Some([
            density,
            friction,
            elasticity,
            friction_weight,
            elasticity_weight,
        ]),
        _ => None,
    }
}

fn material_service(dom: &WeakDom) -> Option<&Instance> {
    dom.root_refs()
        .iter()
        .filter_map(|&root| dom.get(root))
        .find(|instance| instance.class() == "MaterialService")
}

#[cfg(test)]
mod tests;
