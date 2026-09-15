# Roblox Binary & Place Parser — Technical Specs & Architecture

Technical architecture guide in Rust for developing a Roblox archive parser (`.rbxm` / `.rbxl`).

---

## 1. Automated CI Pipeline (API-Dump Fetcher)

To ensure compatibility with the latest Roblox binary format version, the CI must check the latest Studio version daily and update the local API Dump file.

```yaml
name: Sync Roblox API Dump

on:
  schedule:
    # Daily execution at midnight
    - cron: '0 0 * * *'
  workflow_dispatch:

jobs:
  update-api-dump:
    runs-on: ubuntu-latest
    steps:
      - name: Checkout repository
        uses: actions/checkout@v4

      - name: Fetch latest Roblox Studio version
        id: get_version
        run: |
          VERSION=$(curl -s https://setup.rbxcdn.com/versionQTStudio)
          echo "rbx_version=$VERSION" >> $GITHUB_OUTPUT
          echo "Latest Roblox version: $VERSION"

      - name: Download latest API-Dump.json
        run: |
          VERSION="${{ steps.get_version.outputs.rbx_version }}"
          curl -s "https://setup.rbxcdn.com/${VERSION}-API-Dump.json" -o assets/API-Dump.json

      - name: Check for changes
        id: git_status
        run: |
          if git diff --quiet assets/API-Dump.json; then
            echo "changed=false" >> $GITHUB_OUTPUT
          else
            echo "changed=true" >> $GITHUB_OUTPUT
          fi

      - name: Commit and push update
        if: steps.git_status.outputs.changed == 'true'
        run: |
          git config --global user.name "github-actions[bot]"
          git config --global user.email "github-actions[bot]@users.noreply.github.com"
          git commit -am "chore(deps): update Roblox API Dump to ${{ steps.get_version.outputs.rbx_version }}"
          git push
```

> **Official Reference:**
> - [Roblox Setup CDN (versionQTStudio)](https://setup.rbxcdn.com/versionQTStudio)
> - [GitHub Actions — Workflow syntax for GitHub Actions](https://docs.github.com/en/actions/writing-workflows)

---

## 2. Binary Format Architecture (`.rbxm` / `.rbxl`)

The Roblox binary format relies on a strict magic header followed by compressed chunks (typically LZ4).

### File Structure & Header
- **Magic Header**: `<roblox!` (8 bytes: `[0x3C, 0x72, 0x6F, 0x62, 0x6C, 0x6F, 0x78, 0x21]`)
- **Signature**: `[0x89, 0xFF, 0x0D, 0x0A, 0x1A, 0x0A]`
- **Version**: Binary format version (uint16, most commonly `0x0000`).
- **Types Count (`numInstanceTypes`)**: `i32` (Number of unique instance types).
- **Instances Count (`numInstances`)**: `i32` (Total number of objects in the file).

### Core Chunk Types
1. **`INST`**: Defines instance types (e.g., `Part`, `Script`) and their associated IDs.
2. **`PROP`**: Contains serialized properties for each instance (Position, Name, CFrame, etc.).
3. **`PRNT`**: Defines parent-child relationships (workspace hierarchy).
4. **`END\0`**: Marks the end of the binary archive.

> **Technical Reference & Offsets:**
> - [MrSprinkleToes / rbxBinaryParser (C++ Implementation)](https://github.com/MrSprinkleToes/rbxBinaryParser/tree/master/src)
> - Refer to `BinaryFormat.h`, `ChunkINST.cpp`, `ChunkPROP.cpp`, and `ChunkPRNT.cpp` for low-level byte alignments and decompression offsets.

---

## 3. Modular Codebase Architecture

To maintain compilation speed and parser readability, split the codebase into a sub-crate/module isolation pattern.

### Recommended Crate Layout
```text
crates/
├── rbx_dom/              # In-memory representation of instance trees
├── rbx_reflection/       # API-Dump.json parser (types & signatures)
├── rbx_binary/           # Binary deserialization logic (INST, PROP, PRNT)
│   ├── src/
│   │   ├── header.rs
│   │   ├── chunks/
│   │   │   ├── mod.rs
│   │   │   ├── inst.rs
│   │   │   ├── prop.rs
│   │   │   └── prnt.rs
│   │   └── deserializer.rs
└── rbx_parser_cli/       # CLI application for extraction and inspection
```

### Zero-Copy Parsing Strategy
Use the **`nom`** or **`zerocopy`** crate to handle buffer slices without unnecessary reallocation before LZ4 decompression.

```rust
// Minimal header validation without allocation
pub struct BinaryHeader<'a> {
    pub magic: &'a [u8; 8],
    pub num_types: i32,
    pub num_instances: i32,
}

pub fn parse_header(input: &[u8]) -> Result<BinaryHeader, ParseError> {
    if input.len() < 16 || &input[0..8] != b"<roblox!" {
        return Err(ParseError::InvalidMagicHeader);
    }
    
    // Read LE integers directly from byte slice
    let num_types = i32::from_le_bytes(input[12..16].try_into()?);
    let num_instances = i32::from_le_bytes(input[16..20].try_into()?);

    Ok(BinaryHeader {
        magic: b"<roblox!",
        num_types,
        num_instances,
    })
}
```

> **Official References:**
> - [The Cargo Book — Workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html)
> - [nom crate — Parser Combinators](https://docs.rs/nom)

---

## 4. Pragmatic Human Comments for Binary Parsers

Binary format parsing logic handles complex edge cases. Apply comments focused on the "why".

### Guidelines
1. **Link Offsets to Specs**: Link original binary structures in code comments (`// Offset 0x08 -> LZ4 Compressed Block`).
2. **Document Endianness & Bit-packing**: Document complex data transformations (Interleaving, ZigZag encoding).
3. **Safety First**: Always justify the absence of overflow checking with prior slice size validation (`SAFETY:`).

```rust
/// Deserializes a LZ4-compressed chunk block.
///
/// Roblox packs chunk metadata with interleaved byte arrays for better compression ratios.
/// Decompression must reconstruct the original array order before type casting.
pub fn decompress_chunk<'a>(raw_data: &'a [u8], uncompressed_size: usize) -> Result<Vec<u8>, ChunkError> {
    // Note: Empty chunks have an uncompressed_size of 0 and no LZ4 payload.
    if uncompressed_size == 0 {
        return Ok(Vec::new());
    }

    // Workaround: Roblox binary data uses LZ4 block format, NOT the LZ4 frame format.
    lz4_flex::decompress(raw_data, uncompressed_size)
        .map_err(|_| ChunkError::DecompressionFailed)
}
```

---

## 5. Verification & Testing Tools

Every parser modification must be validated by unit tests integrating the `API-Dump.json` file.

```bash
# Run unit tests using local assets
cargo test --package rbx_binary

# Check memory leaks or improper slice usage
cargo clippy --workspace -- -D warnings
```

> **Official Reference:**
> - [The Rust Programming Language — Chapter 11: Writing Automated Tests](https://doc.rust-lang.org/book/ch11-00-writing-automated-tests.html)
