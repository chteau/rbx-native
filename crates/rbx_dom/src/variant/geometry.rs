//! Geometric and dimensional property value types.

/// A 2D vector (x, y coordinates).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vector2Data {
    pub x: f32,
    pub y: f32,
}

/// A 3D vector (x, y, z coordinates).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vector3Data {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// An RGB color with components normalized to the 0..1 range.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color3Data {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

/// An axis-aligned rectangle delimited by two corners.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub min: Vector2Data,
    pub max: Vector2Data,
}

/// A position combined with a 3×3 rotation matrix stored row-major.
///
/// Shared by the `CFrame` and `OptionalCFrame` property types, which differ only
/// in whether the value may be absent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CFrameData {
    pub position: Vector3Data,
    pub rotation: [f32; 9],
}

/// A 1D dimension that combines a scale factor and an offset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UDim {
    pub scale: f32,
    pub offset: i32,
}

/// A 2D dimension specified with two `UDim` components.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UDim2 {
    pub x: UDim,
    pub y: UDim,
}

/// Physics properties for a part: either default or custom with five parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PhysicalProperties {
    Default,
    Custom {
        density: f32,
        friction: f32,
        elasticity: f32,
        friction_weight: f32,
        elasticity_weight: f32,
    },
}
