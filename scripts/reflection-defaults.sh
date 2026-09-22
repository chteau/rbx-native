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
  # was +inf, -inf or NaN. Its msgpack database at the same commit
  # (rbx_reflection_database/database.msgpack) keeps the sign; these are
  # every one it has, spelled as strings since JSON has no number for them.
  # Any other `null` is dropped rather than guessed.
  def infinite: {
    AlignOrientation: {MaxAngularVelocity: {Float32: "inf"}},
    AlignPosition: {MaxVelocity: {Float32: "inf"}},
    BillboardGui: {MaxDistance: {Float32: "inf"}},
    CylindricalConstraint: {MotorMaxAcceleration: {Float32: "inf"}},
    LineForce: {MaxForce: {Float32: "inf"}},
    PrismaticConstraint: {MotorMaxAcceleration: {Float32: "inf"}},
    SpringConstraint: {MaxForce: {Float32: "inf"}},
    TorsionSpringConstraint: {MaxTorque: {Float32: "inf"}},
    UISizeConstraint: {MaxSize: {Vector2: ["inf", "inf"]}},
    WrapTextureTransfer: {
      UVMaxBound: {Vector2: ["-inf", "-inf"]},
      UVMinBound: {Vector2: ["inf", "inf"]}
    }
  };
  # The one real `null` is an absent `OptionalCFrame`: no CFrame at all.
  def exact: has("OptionalCFrame") or ([.[] | .. | nulls] | length == 0);
  def saved: . == "Serializes" or (type == "object" and has("SerializesAs"));
  {
    Source: ("rojo-rbx/rbx-dom@" + $rev),
    License: "MIT, Copyright (c) 2018-2025 The Rojo Developers",
    Version,
    Classes: (.Classes | with_entries(.key as $class | .value |= ({
      Aliases: (.Properties
        | with_entries(select(.value.Kind.Alias))
        | map_values(.Kind.Alias.AliasFor)),
      # Each property an alias names that is never saved, so never loaded
      # either: `PackageId`, which only migrates to `PackageContent`. A
      # stored `PackageIdSerialize` reads as it, but keeps its own name.
      NotLoaded: (.Properties as $properties
        | [$properties[] | .Kind.Alias.AliasFor // empty
          | select($properties[.].Kind.Canonical.Serialization | saved | not)]
        | unique),
      SerializesAs: (.Properties
        | with_entries(select(.value.Kind.Canonical.Serialization | type == "object" and has("SerializesAs")))
        | map_values(.Kind.Canonical.Serialization.SerializesAs)),
      Defaults: ((.DefaultProperties // {})
        | with_entries(if [.value | .. | nulls] | length > 0
            then .value = (infinite[$class][.key] // .value) else . end)
        | with_entries(select(.value | supported and exact)))
    } | with_entries(select(.value | length > 0)))) | with_entries(select(.value != {})))
  }
  # One class per line: small enough to diff, where one line for the whole
  # file or one per number would not be.
  | "{\"Source\": \(.Source | tojson), \"License\": \(.License | tojson), \"Version\": \(.Version | tojson), \"Classes\": {",
    (.Classes | to_entries | map("\(.key | tojson): \(.value | tojson)") | join(",\n")),
    "}}"' > assets/reflection-defaults.json
