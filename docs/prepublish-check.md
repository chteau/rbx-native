# Pre-publish check

One pass over a place before it goes to Roblox, listing what would break or
look wrong once players join. In the editor it's View › Pre-publish Check
(or the Pre-publish tile on the Home tab), and File › Save to Roblox /
Publish to Roblox runs it before the confirmation. For CI it's
`rbxaudit --prepublish`.

Every finding has a severity:

- **error** (High): fix it before publishing. `rbxaudit` exits 1 when there
  is one.
- **warning** (Medium): probably a bug, worth a look.
- **info** (Low): a note or a cleanup.

Clicking a finding in the dock opens the script at its line, or selects the
instance it's about.

## Rules

| Rule id | What it finds | Severity |
| --- | --- | --- |
| `broken-asset` | A content property (`MeshId`, `TextureID`, `Image`, `SoundId`, `AnimationId`, `Decal.Texture`, `MeshContent`, and every other property the API dump types as content) that isn't an asset reference Roblox can load, like `rbxassetid://abc` or id `0`. | error |
| | An id Roblox's asset service says doesn't exist. | error |
| | An id the asset service refuses (moderated, or not shared with anyone). | warning |
| | An empty id where the class needs one: a `MeshPart`'s mesh, a `SpecialMesh` of type `FileMesh`, an `Animation`. | warning |
| | An empty `Sound.SoundId`, `Decal.Texture` or `Texture.Texture` (often filled in by a script, so only a note). | info |
| `assets-not-checked` | Asset ids the asset service couldn't be asked about, because you're offline. Shown once with the count; run the check again online. | info |
| `deprecated-api` | `wait`, `spawn`, `delay` (use the `task` library), `Instance.new(class, parent)` (set `Parent` last), `:connect` / `:disconnect`, `game.Workspace` (use `workspace`), and methods the API dump tags `Deprecated`, like `FindPartOnRay` or `SetPrimaryPartCFrame`. | info |
| `unanchored-parts` | Unanchored parts in the Workspace that aren't welded or constrained to an anchored part and aren't held by a mover, so they fall when the game starts. One finding per top-level model with the count. | warning |
| `datastore-without-pcall` | DataStore and MemoryStore calls (`GetAsync`, `SetAsync`, `UpdateAsync`, `IncrementAsync`, `RemoveAsync`, `GetSortedAsync`, MemoryStore maps and queues) that aren't inside `pcall`, `xpcall` or a `Promise`. | warning |
| `heavy-geometry` | A `MeshPart` over Roblox's 20,000-triangle limit. | error |
| | A union over 10,000 triangles. | warning |
| `place-budget` | More than 30,000 parts or 250,000 instances in the place. | warning |
| `streaming-index` | With `Workspace.StreamingEnabled` on, client code that reaches a part with `.` (`workspace.Map.Door`) instead of `WaitForChild`. | info |

The Security Audit's rules (unvalidated remote arguments, remotes without a
server listener, bad `require` targets and the rest) are part of the check
too, with the same severities as in the Security Audit dock.

## Thresholds

| Threshold | Value | Where it comes from |
| --- | --- | --- |
| MeshPart triangles | 20,000 | Roblox's import limit for a single mesh. A mesh over it was never meant to ship. |
| Union triangles | 10,000 | Our budget. Roblox has no cap, but CSG this heavy costs memory and physics on every device; a mesh is the better tool. |
| Parts per place | 30,000 | Our budget. Roblox sets no cap; past this, low-end phones drop frames and joining takes long. |
| Instances per place | 250,000 | Our budget, same reason. |

## How each rule decides

**Asset ids.** The check collects every asset id in the place and asks
Roblox's asset delivery about each one with a plain read-only GET (no key,
no cookie, nothing downloaded): "not found" means missing, "authentication
required" means it exists. Answers are kept for the session, so a re-run
only asks about new ids. When the first request gets no answer the rest
aren't asked and the ids are reported as not checked, instead of waiting
on the network for each. In the editor, automatic re-runs don't ask again
after that; Check now does.

**Mesh triangles** are counted from the mesh in the local asset cache (the
editor puts every mesh it draws there), never downloaded for the check. A
mesh that isn't cached isn't measured. Unions use the `TriangleCount`
Studio saves with them.

**Deprecated methods.** When the receiver is something the check can follow
(`workspace`, `game:GetService(...)`, a path into the place, a local
assigned one of those), the method is looked up on that class in the API
dump. Otherwise only methods deprecated on every class that has them count
(`FindPartOnRay`, `isA`, but not `Remove`, which is current on `Tween`), and
not when the place's own scripts define a method of that name
(`function Mover:move()`), or the receiver is `self`. A script that declares
its own `wait`, `spawn` or `delay` isn't flagged for calling it.

**Falling parts.** Parts are joined through welds, `Motor6D`s, other joints
and constraints (`NoCollisionConstraint` excepted). A group is held when one
of its parts is anchored, or it has a one-attachment mover (`AlignPosition`,
`LinearVelocity`) or a legacy body mover. Characters (a model with a
`Humanoid` or `AnimationController`), tools and accessories are skipped.
A loose part resting on the ground doesn't really fall far, so treat the
warning as "is this meant to move?".

**DataStore calls.** `SetAsync`, `UpdateAsync` and the other store-only
methods count wherever they're called; `GetAsync`, which `HttpService` has
too, counts only on something assigned from `GetDataStore`,
`GetOrderedDataStore`, `GetGlobalDataStore` or a MemoryStore getter. A call
is protected when it's inside a function passed to `pcall`, `xpcall`,
`Promise.new`, `Promise.try` or `Promise.defer`, or inside a named function
that one of those calls, directly or through other named functions.

**Streaming.** Only in client scripts, only when `StreamingEnabled` is on,
and only for a `.` chain from `workspace` that reaches a `BasePart`, or an
`Atomic` model, in the place. Folders and non-atomic model containers are
sent at join, so indexing them is fine.

## Before publishing

Save to Roblox and Publish to Roblox run the check first. While it runs the
dialog has Skip check. If it finds errors or warnings it lists the worst
five with Review (opens the dock, nothing is uploaded) and Continue (goes on
to the usual confirmation). Notes alone don't stop it: the confirmation
comes straight up.

Tick "Don't ask again for this place" to skip the check for that place from
then on. The dock's "Ask before publishing" box (shown when the file is
linked to a Roblox place) turns it back on.

## CI

```sh
rbxaudit --prepublish place.rbxl            # one line per finding, then a summary
rbxaudit --prepublish --json place.rbxl     # one JSON object
rbxaudit --prepublish --offline place.rbxl  # don't ask the asset service
```

The JSON has `findings` (each with `severity`, `check`, `title`, `path`,
`line` or `null`, and `message`) and `summary` (`high`, `medium`, `low`).
The exit code is 1 when there's at least one error, 0 otherwise, and 2 for
bad usage or a file that can't be read. Without `--prepublish`, `rbxaudit`
runs only the Security Audit, as before.
