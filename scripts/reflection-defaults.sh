#!/usr/bin/env bash
# Regenerates assets/reflection-defaults.json from rbx-dom's reflection
# database (MIT, https://github.com/rojo-rbx/rbx-dom). Roblox's API dump
# records property types but neither a class's default values nor which
# properties are one value saved under another name (`Size` as `size`,
# `Color` as `Color3uint8`); rbx-dom records both, generated from Studio
# itself. This keeps only those two things, for the value types
# `rbx_reflection` converts (see its `extras` module).
#
# Pinned to one rbx-dom commit so a rerun is reproducible; bump RBX_DOM_REV
# to take a newer Studio's data.
set -euo pipefail
cd "$(dirname "$0")/.."
RBX_DOM_REV="${RBX_DOM_REV:-bc75fd68cbf5403910e94627492a883701e259d8}"
URL="https://raw.githubusercontent.com/rojo-rbx/rbx-dom/$RBX_DOM_REV/rbx_dom_lua/src/database.json"
curl -fsSL "$URL" | jq -r --arg rev "$RBX_DOM_REV" '
  def supported: keys[0] | IN(
    "Bool", "Int32", "Int64", "Float32", "Float64", "String", "Enum",
    "BrickColor", "Color3", "Color3uint8", "Vector2", "Vector3",
    "Vector3int16", "CFrame", "OptionalCFrame", "UDim", "UDim2", "Rect",
    "NumberRange", "NumberSequence", "ColorSequence", "PhysicalProperties",
    "Font", "Faces", "Axes", "Ray", "Content", "SecurityCapabilities");
  # Not `ContentId`: the DOM holds one as a `String` or a `Content` depending
  # on which file format it came from, so a default has no one right shape.
  # rbx-dom writes a non-finite float as `null`, which cannot say whether it
  # was +inf, -inf or NaN; such a default is dropped rather than guessed.
  # The one real `null` is an absent `OptionalCFrame`: no CFrame at all.
  def exact: has("OptionalCFrame") or ([.[] | .. | nulls] | length == 0);
  {
    Source: ("rojo-rbx/rbx-dom@" + $rev),
    License: "MIT, Copyright (c) 2018-2025 The Rojo Developers",
    Version,
    Classes: (.Classes | map_values({
      # An alias is kept only when its canonical is itself saved, under its
      # own name or another: loading renames an alias to what gets saved,
      # and a canonical that only migrates (`PackageIdSerialize` to
      # `PackageId`) or never serializes is a name Studio will not load.
      Aliases: (.Properties as $properties | $properties
        | with_entries(select(.value.Kind.Alias
            and ($properties[.value.Kind.Alias.AliasFor].Kind.Canonical.Serialization
              | . == "Serializes" or (type == "object" and has("SerializesAs")))))
        | map_values(.Kind.Alias.AliasFor)),
      SerializesAs: (.Properties
        | with_entries(select(.value.Kind.Canonical.Serialization | type == "object" and has("SerializesAs")))
        | map_values(.Kind.Canonical.Serialization.SerializesAs)),
      Defaults: ((.DefaultProperties // {}) | with_entries(select(.value | supported and exact)))
    } | with_entries(select(.value != {}))) | with_entries(select(.value != {})))
  }
  # One class per line: small enough to diff, where one line for the whole
  # file or one per number would not be.
  | "{\"Source\": \(.Source | tojson), \"License\": \(.License | tojson), \"Version\": \(.Version | tojson), \"Classes\": {",
    (.Classes | to_entries | map("\(.key | tojson): \(.value | tojson)") | join(",\n")),
    "}}"' > assets/reflection-defaults.json
