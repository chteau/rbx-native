//! Bitfield property value types (Faces, Axes).

/// Which faces of a part's bounding box are selected.
///
/// The wire format packs these into the low 6 bits of one byte in the order
/// Front, Bottom, Left, Back, Top, Right; that order is preserved here as field
/// declaration order even though it does not read alphabetically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Faces {
    pub front: bool,
    pub bottom: bool,
    pub left: bool,
    pub back: bool,
    pub top: bool,
    pub right: bool,
}

impl Faces {
    /// Decodes the low 6 bits of a wire byte; the top 2 bits are unused padding.
    pub fn from_bits(bits: u8) -> Self {
        Faces {
            front: bits & 0b0000_0001 != 0,
            bottom: bits & 0b0000_0010 != 0,
            left: bits & 0b0000_0100 != 0,
            back: bits & 0b0000_1000 != 0,
            top: bits & 0b0001_0000 != 0,
            right: bits & 0b0010_0000 != 0,
        }
    }
}

/// Which of the X/Y/Z axes are selected.
///
/// The wire format packs these into the low 3 bits of one byte in the order X, Y, Z.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Axes {
    pub x: bool,
    pub y: bool,
    pub z: bool,
}

impl Axes {
    /// Decodes the low 3 bits of a wire byte; the top 5 bits are unused padding.
    pub fn from_bits(bits: u8) -> Self {
        Axes {
            x: bits & 0b001 != 0,
            y: bits & 0b010 != 0,
            z: bits & 0b100 != 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faces_decodes_low_six_bits_in_order() {
        // Front (bit 0) and Top (bit 4) set; top two padding bits ignored.
        let faces = Faces::from_bits(0b1101_0001);
        assert_eq!(
            faces,
            Faces {
                front: true,
                bottom: false,
                left: false,
                back: false,
                top: true,
                right: false,
            }
        );
    }

    #[test]
    fn axes_decodes_low_three_bits_in_order() {
        // X (bit 0) and Z (bit 2) set; padding bits ignored.
        let axes = Axes::from_bits(0b1111_0101);
        assert_eq!(
            axes,
            Axes {
                x: true,
                y: false,
                z: true,
            }
        );
    }
}
