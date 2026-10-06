//! Which spelling Roblox uses for a `Content` column's source tags, settled
//! from a real place rather than from either written source.
//!
//! rbx-dom's `docs/binary.md` types `SourceTypes` as `Array(Enum)` — plain
//! big-endian `u32`s, interleaved — while rbx-dom's own serializer writes
//! them with `write_interleaved_i32_array`, which also zigzags them (so a
//! `Uri` tag `1` lands as `2`). Each tag consumes one entry of its source's
//! pool, so only one reading can make the tag counts match the pool counts
//! the chunk also states: this test reads every `Content` column of a real
//! Studio file both ways and asserts the zigzagged one, which is what
//! `chunks::prop::content` decodes and `serialize::prop::content` writes.
//! rbx-test-files' `models/imagelabel-content/binary.rbxm` (Studio 0.663)
//! is a file that settles it; the places in this project's fixtures hold
//! only empty Content columns, which settle nothing.

use rbx_binary::{parse_header, read_chunks};

const CONTENT_TYPE_ID: u8 = 0x22;

struct Cursor<'a>(&'a [u8]);

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> &'a [u8] {
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        head
    }
    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.take(4).try_into().unwrap())
    }
    fn string(&mut self) -> String {
        let len = self.u32() as usize;
        String::from_utf8_lossy(self.take(len)).into_owned()
    }
    /// `count` big-endian `u32`s, byte-interleaved.
    fn interleaved(&mut self, count: usize) -> Vec<u32> {
        let bytes = self.take(count * 4);
        (0..count)
            .map(|i| u32::from_be_bytes([0, 1, 2, 3].map(|b| bytes[b * count + i])))
            .collect()
    }
}

/// (class.property, raw tags, uri pool length, object pool length) for every
/// `Content` column in `place`.
fn content_columns(place: &[u8]) -> Vec<(String, Vec<u32>, usize, usize)> {
    let (_, body) = parse_header(place).unwrap();
    let mut counts = std::collections::HashMap::new();
    let mut names = std::collections::HashMap::new();
    let mut columns = Vec::new();
    for chunk in read_chunks(body) {
        let chunk = chunk.unwrap();
        let mut cursor = Cursor(&chunk.data);
        match chunk.name_str() {
            "INST" => {
                let class_id = cursor.u32();
                let name = cursor.string();
                cursor.take(1);
                counts.insert(class_id, cursor.u32() as usize);
                names.insert(class_id, name);
            }
            "PROP" => {
                let class_id = cursor.u32();
                let name = cursor.string();
                if cursor.take(1)[0] != CONTENT_TYPE_ID {
                    continue;
                }
                let tags = cursor.interleaved(counts[&class_id]);
                let uri_count = cursor.u32() as usize;
                for _ in 0..uri_count {
                    cursor.string();
                }
                let object_count = cursor.u32() as usize;
                columns.push((
                    format!("{}.{name}", names[&class_id]),
                    tags,
                    uri_count,
                    object_count,
                ));
            }
            _ => {}
        }
    }
    columns
}

fn zigzag(raw: u32) -> u32 {
    ((raw >> 1) as i32 ^ -((raw & 1) as i32)) as u32
}

#[test]
#[ignore = "needs RBX_CONTENT_FIXTURE"]
fn a_real_files_content_tags_are_zigzagged() {
    let path = std::env::var("RBX_CONTENT_FIXTURE")
        .expect("set RBX_CONTENT_FIXTURE to a Studio-saved .rbxm/.rbxl");
    let file = std::fs::read(&path).unwrap();
    let mut with_uris = 0;
    for (name, raw, uris, objects) in content_columns(&file) {
        let count = |tags: &[u32], tag| tags.iter().filter(|&&t| t == tag).count();
        println!("{name}: {uris} uris, {objects} objects, raw tags {raw:?}");
        let zigzagged: Vec<u32> = raw.iter().map(|&t| zigzag(t)).collect();
        assert_eq!(
            (count(&zigzagged, 1), count(&zigzagged, 2)),
            (uris, objects),
            "{name}: zigzagged tags disagree with the pools"
        );
        if uris > 0 {
            with_uris += 1;
            assert_ne!(count(&raw, 1), uris, "{name}: plain tags also fit");
        }
    }
    assert!(
        with_uris > 0,
        "the file has no Uri-sourced Content, so it settles nothing"
    );
}
