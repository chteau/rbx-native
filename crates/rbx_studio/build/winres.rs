//! A compiled resource file (`.res`) for `rbxstudio.exe`, written by hand so
//! the build needs neither `rc.exe` nor another crate: the icon, and the
//! version block Explorer's Properties › Details tab reads.

const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
const RT_VERSION: u16 = 16;

/// The `.res` holding one icon group (`ICON 1`) with every image of `ico`,
/// then `VS_VERSION_INFO` for `version` ("major.minor.patch").
pub(crate) fn res(ico: &[u8], version: &str) -> Vec<u8> {
    let mut out = Vec::new();
    // A `.res` file starts with an empty entry.
    entry(&mut out, 0, 0, 0, &[]);
    icon(&mut out, ico);
    entry(&mut out, RT_VERSION, 1, 0x0030, &version_info(version));
    out
}

fn icon(out: &mut Vec<u8>, ico: &[u8]) {
    let u16_at = |at: usize| u16::from_le_bytes([ico[at], ico[at + 1]]);
    let u32_at = |at: usize| u32::from_le_bytes(ico[at..at + 4].try_into().unwrap());
    assert_eq!(
        (u16_at(0), u16_at(2)),
        (0, 1),
        "rbxstudio.ico is not an .ico"
    );
    let count = u16_at(4);
    let mut group = Vec::new();
    group.extend_from_slice(&[0, 0, 1, 0]);
    group.extend_from_slice(&count.to_le_bytes());
    for index in 0..count {
        let at = 6 + 16 * usize::from(index);
        let size = u32_at(at + 8) as usize;
        let offset = u32_at(at + 12) as usize;
        let id = index + 1;
        entry(out, RT_ICON, id, 0x1010, &ico[offset..offset + size]);
        // GRPICONDIRENTRY: the ICONDIRENTRY's first 12 bytes, then the id.
        group.extend_from_slice(&ico[at..at + 12]);
        group.extend_from_slice(&id.to_le_bytes());
    }
    entry(out, RT_GROUP_ICON, 1, 0x1030, &group);
}

/// `VS_VERSIONINFO`: the fixed binary version, then US-English/Unicode
/// strings for the fields Explorer shows.
fn version_info(version: &str) -> Vec<u8> {
    let mut parts = version
        .split(['.', '-', '+'])
        .map(|part| part.parse::<u32>().unwrap_or(0));
    let (major, minor, patch) = (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    );
    let ms = (major << 16) | (minor & 0xFFFF);
    let ls = patch << 16;
    let mut fixed = Vec::new();
    for value in [
        0xFEEF_04BD, // signature
        0x0001_0000, // structure version
        ms,
        ls, // file version
        ms,
        ls,          // product version
        0x3F,        // flags mask
        0,           // flags
        0x0004_0004, // VOS_NT_WINDOWS32
        1,           // VFT_APP
        0,
        0,
        0,
    ] {
        fixed.extend_from_slice(&u32::to_le_bytes(value));
    }
    let strings: Vec<Vec<u8>> = [
        ("CompanyName", "RbxNative"),
        ("FileDescription", "RbxNative"),
        ("FileVersion", version),
        ("InternalName", "rbxstudio"),
        ("OriginalFilename", "RbxNative.exe"),
        ("ProductName", "RbxNative"),
        ("ProductVersion", version),
    ]
    .iter()
    .map(|(key, value)| {
        let text = utf16(value);
        node(key, (text.len() / 2) as u16, 1, &text, &[])
    })
    .collect();
    let table = node("040904B0", 0, 1, &[], &strings);
    let string_info = node("StringFileInfo", 0, 1, &[], &[table]);
    let translation = [0x09, 0x04, 0xB0, 0x04];
    let var = node("Translation", 4, 0, &translation, &[]);
    let var_info = node("VarFileInfo", 0, 1, &[], &[var]);
    node(
        "VS_VERSION_INFO",
        fixed.len() as u16,
        0,
        &fixed,
        &[string_info, var_info],
    )
}

/// A null-terminated UTF-16LE string.
fn utf16(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect()
}

/// One version-resource block: `wLength`, `wValueLength`, `wType`, the key,
/// the value and the children, each started on a 4-byte boundary.
fn node(key: &str, value_length: u16, kind: u16, value: &[u8], children: &[Vec<u8>]) -> Vec<u8> {
    let mut out = vec![0, 0];
    out.extend_from_slice(&value_length.to_le_bytes());
    out.extend_from_slice(&kind.to_le_bytes());
    out.extend_from_slice(&utf16(key));
    pad(&mut out);
    out.extend_from_slice(value);
    for child in children {
        pad(&mut out);
        out.extend_from_slice(child);
    }
    let length = out.len() as u16;
    out[..2].copy_from_slice(&length.to_le_bytes());
    out
}

fn pad(out: &mut Vec<u8>) {
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
}

/// One `RESOURCEHEADER` with a numeric type and name, then `data`, padded
/// to 4 bytes.
fn entry(out: &mut Vec<u8>, kind: u16, name: u16, flags: u16, data: &[u8]) {
    let language: u16 = if kind == 0 { 0 } else { 0x0409 };
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&32u32.to_le_bytes());
    out.extend_from_slice(&[0xFF, 0xFF]);
    out.extend_from_slice(&kind.to_le_bytes());
    out.extend_from_slice(&[0xFF, 0xFF]);
    out.extend_from_slice(&name.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&language.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(data);
    pad(out);
}
