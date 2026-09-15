//! The ASCII `version X.YY` line that opens every mesh file, all versions.

use crate::error::MeshError;

// Scanning for the terminator only inside this window keeps a large non-mesh
// input from being walked end to end before it is rejected.
const MAX_VERSION_LINE: usize = 32;

/// Splits the leading version line off `bytes`, returning `((major, minor), body)`.
///
/// `version 1.00` files terminate the line with CRLF and every later version
/// observed uses a bare LF, so the trailing `\r` is optional here.
pub(crate) fn parse_version(bytes: &[u8]) -> Result<((u8, u8), &[u8]), MeshError> {
    let window = &bytes[..bytes.len().min(MAX_VERSION_LINE)];
    let newline = window
        .iter()
        .position(|&byte| byte == b'\n')
        .ok_or(MeshError::MissingVersionLine)?;

    let line = window[..newline]
        .strip_suffix(b"\r")
        .unwrap_or(&window[..newline]);
    let text = std::str::from_utf8(line).map_err(|_| MeshError::MissingVersionLine)?;
    let digits = text
        .strip_prefix("version ")
        .ok_or(MeshError::MissingVersionLine)?;

    let (major, minor) = digits
        .split_once('.')
        .ok_or_else(|| MeshError::MalformedVersionLine(text.to_owned()))?;
    let parsed = |part: &str| {
        part.parse::<u8>()
            .map_err(|_| MeshError::MalformedVersionLine(text.to_owned()))
    };

    Ok(((parsed(major)?, parsed(minor)?), &bytes[newline + 1..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_bare_newline_header() {
        let (version, body) = parse_version(b"version 4.01\nBODY").unwrap();
        assert_eq!(version, (4, 1));
        assert_eq!(body, b"BODY");
    }

    #[test]
    fn parses_the_crlf_header_written_by_version_1() {
        let (version, body) = parse_version(b"version 1.00\r\n12\r\n").unwrap();
        assert_eq!(version, (1, 0));
        assert_eq!(body, b"12\r\n");
    }

    #[test]
    fn keeps_the_minor_number_as_written() {
        assert_eq!(parse_version(b"version 7.00\n").unwrap().0, (7, 0));
        assert_eq!(parse_version(b"version 3.01\n").unwrap().0, (3, 1));
    }

    #[test]
    fn rejects_input_without_a_version_line() {
        assert!(matches!(
            parse_version(b"<roblox!"),
            Err(MeshError::MissingVersionLine)
        ));
        assert!(matches!(
            parse_version(b""),
            Err(MeshError::MissingVersionLine)
        ));
        assert!(matches!(
            parse_version(b"not a mesh\n"),
            Err(MeshError::MissingVersionLine)
        ));
    }

    #[test]
    fn rejects_a_version_line_that_is_not_two_numbers() {
        assert!(matches!(
            parse_version(b"version four\n"),
            Err(MeshError::MalformedVersionLine(_))
        ));
        assert!(matches!(
            parse_version(b"version 4.x\n"),
            Err(MeshError::MalformedVersionLine(_))
        ));
        assert!(matches!(
            parse_version(b"version 999.0\n"),
            Err(MeshError::MalformedVersionLine(_))
        ));
    }

    #[test]
    fn does_not_scan_a_huge_input_for_a_newline() {
        let mut bytes = vec![b'x'; 1 << 20];
        bytes.push(b'\n');
        assert!(matches!(
            parse_version(&bytes),
            Err(MeshError::MissingVersionLine)
        ));
    }
}
