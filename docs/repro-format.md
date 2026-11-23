# The `.rbxrepro` file

A repro is one recorded playtest packed with what it takes to replay it on
another computer: Playtests dock › right-click a session › **Export repro…**.
It opens with File › Open, a drop on the editor window, or
`rbxstudio file.rbxrepro`, read-only in a window of its own. The code is
`crates/rbx_studio/src/playtests/repro.rs`.

## Layout (format version 1)

A plain zip archive (Deflate), so any unzip tool can look inside:

| Entry              | What it is |
|--------------------|------------|
| `repro.json`       | The manifest, below. |
| `recording.rbxrec` | The recording, in the same binary format as the Playtests dock's own files (`playtests/format.rs`, `RBXREC` magic, its own version). |
| `place.rbxl`       | The place it was recorded against, binary, as it was in the editor at export time. Missing when the sender left it out. |

`repro.json`:

```json
{
  "format": "rbxrepro",
  "version": 1,
  "kind": "rbxrec",
  "editor_version": "0.2.2",
  "created": 1763900000,
  "note": "Falls through the floor after the second checkpoint",
  "recording": "recording.rbxrec",
  "place": {
    "name": "Obby",
    "file": "place.rbxl",
    "sha256": "<64 hex digits of the place bytes>",
    "size": 1048576,
    "universe_id": 123,
    "place_id": 456
  }
}
```

- `format` is always `"rbxrepro"`; anything else is not a repro.
- `version` is this layout's number. A reader refuses a higher one and says
  which RbxNative made it. New optional fields don't bump it: readers ignore
  fields they don't know.
- `kind` names the recording inside. `"rbxrec"` is the only one so far.
  Local Run's deterministic `.rbxnrec` will get its own kind; a reader that
  doesn't know a kind says so rather than guessing.
- `created` is Unix seconds. `note` is the sender's own words (may be empty).
- `place.sha256` and `place.size` are always set, from the place's binary
  bytes, whether or not they are embedded. `place.file` is `null` when the
  place was left out, which the export dialog only allows for a published
  place (`universe_id`/`place_id` set).

## Opening

- The place is written to a temporary folder and that copy is opened, so
  nothing in the repro window reaches anyone's real place. Save is refused
  (Save to File As… keeps a copy, and that copy is then an ordinary place).
- No embedded place: the newest Recent entry with the same `place_id` is
  copied in. If its bytes don't hash to `place.sha256`, the Playtests dock
  and Output say it isn't the same file as the one recorded. With no local
  copy it replays over an empty Baseplate and says so.
- Another editor version: a note says which one made it.
- A file that isn't a repro, a newer format, an unknown kind, an
  unreadable recording or a place that doesn't match its hash: nothing
  opens, and the reason is printed (command line) or shown in Output.
