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
RBX_DOM_REV="${RBX_DOM_REV:-dbec31dbe6de9bc7d2b6dd78c35bb67b9236ebda}"
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
  # Properties rbx-dom still records under a legacy saved name that current
  # Studio no longer writes: a place Studio 0.744 saves holds, for a `Sound`,
  # `RollOffMinDistance` and `RollOffMaxDistance` (the API dump marks both
  # CanSave), not `EmitterSize` and `xmlRead_MaxDistance_3`. The legacy names
  # stay aliases, so a file holding them still reads.
  def saved_as_own_name: {Sound: ["RollOffMinDistance", "RollOffMaxDistance"]};
  # Old spellings Studio still loads that rbx-dom records as properties of
  # their own: a MeshPart saved around 2016 holds its mesh as `MeshID`
  # (the "Tree" by SheriffTaco on the Creator Store does), and Studio shows it.
  # `MeshSize` is what scripts and the Properties panel read of the size a
  # MeshPart saves as `InitialSize` (Studio 0.748 reads them equal on every
  # Creator Store mesh); rbx-dom keeps the two apart, so `MeshSize` read 0.
  def legacy_aliases: {MeshPart: {MeshID: "MeshId", MeshSize: "InitialSize"}};
  # Classes the pinned rbx-dom predates, with the defaults a fresh
  # `Instance.new` reads back in Studio 0.748 (version-8d7a1009c17e4ff8).
  # A newer pin that has the class wins over these.
  def seq($v): {keypoints: [{time: 0.0, value: $v, envelope: 0.0}, {time: 1.0, value: $v, envelope: 0.0}]};
  def measured: {
    ParticleEmitter2D: {Defaults: {
      Acceleration: {Vector2: [0.0, 0.0]}, Blend: {Float32: 0.0}, Brightness: {Float32: 1.0},
      Color: {ColorSequence: {keypoints: [{time: 0.0, color: [1.0, 1.0, 1.0]}, {time: 1.0, color: [1.0, 1.0, 1.0]}]}},
      Drag: {Float32: 0.0}, EmissionAngle: {Float32: 0.0}, EmissionMode: {Enum: 1},
      EmissionOffset: {Vector2: [0.0, 0.0]}, EmissionSpread: {Vector2: [0.0, 0.0]}, Enabled: {Bool: true},
      FlipbookBlendFrames: {Bool: true}, FlipbookFramerate: {NumberRange: [1.0, 1.0]},
      FlipbookLayout: {Enum: 0}, FlipbookMode: {Enum: 0}, FlipbookSizeX: {Int32: 1}, FlipbookSizeY: {Int32: 1},
      FlipbookStartRandom: {Bool: false}, Lifetime: {NumberRange: [5.0, 10.0]}, LightInfluence: {Float32: 0.0},
      LockedToParent: {Bool: false}, Rate: {Float32: 20.0}, RotSpeed: {NumberRange: [0.0, 0.0]},
      Rotation: {NumberRange: [0.0, 0.0]}, Size: {NumberSequence: seq(32.0)}, Speed: {NumberRange: [64.0, 64.0]},
      SpreadAngle: {Float32: 0.0}, Squash: {NumberSequence: seq(0.0)},
      TextureContent: {Content: {Uri: "rbxasset://textures/particles/sparkles_main.dds"}},
      TimeScale: {Float32: 1.0}, Transparency: {NumberSequence: seq(0.0)}, ZIndex: {Int32: 1}
    }}
  };
  # Properties the pinned rbx-dom predates on classes it has, as a new
  # Camera and Play's camera read them back in Studio 0.748, Orthographic
  # Camera beta on or off. rbx-dom's own value wins.
  def measured_properties: {
    Camera: {ProjectionType: {Enum: 0}, OrthographicSize: {Float32: 35.0}, OrthographicMode: {Enum: 0}}
  };
  {
    Source: ("rojo-rbx/rbx-dom@" + $rev),
    License: "MIT, Copyright (c) 2018-2025 The Rojo Developers",
    Version,
    Classes: (.Classes | with_entries(.key as $class | .value |= ({
      Aliases: ((.Properties
        | with_entries(select(.value.Kind.Alias))
        | map_values(.Kind.Alias.AliasFor)) + (legacy_aliases[$class] // {})),
      # Each property an alias names that is never saved, so never loaded
      # either: `PackageId`, which only migrates to `PackageContent`. A
      # stored `PackageIdSerialize` reads as it, but keeps its own name.
      NotLoaded: (.Properties as $properties
        | [$properties[] | .Kind.Alias.AliasFor // empty
          | select($properties[.].Kind.Canonical.Serialization | saved | not)]
        | unique),
      SerializesAs: (.Properties
        | with_entries(select(.value.Kind.Canonical.Serialization | type == "object" and has("SerializesAs")))
        | map_values(.Kind.Canonical.Serialization.SerializesAs)
        # Another property saved under one of those legacy names
        # (`MaxDistance`) now saves under the property that took it back.
        | (with_entries(select(.key | IN(saved_as_own_name[$class][]?)))
            | with_entries({key: .value, value: .key})) as $moved
        | map_values($moved[.] // .)
        | delpaths([saved_as_own_name[$class][]? | [.]])),
      Defaults: ((measured_properties[$class] // {}) + (.DefaultProperties // {})
        | with_entries(if [.value | .. | nulls] | length > 0
            then .value = (infinite[$class][.key] // .value) else . end)
        | with_entries(select(.value | supported and exact)))
    } | with_entries(select(.value | length > 0)))) | with_entries(select(.value != {})))
  }
  | .Classes |= (. as $have | . + (measured | with_entries(select(.key as $class | $have | has($class) | not))))
  # One class per line: small enough to diff, where one line for the whole
  # file or one per number would not be.
  | "{\"Source\": \(.Source | tojson), \"License\": \(.License | tojson), \"Version\": \(.Version | tojson), \"Classes\": {",
    (.Classes | to_entries | map("\(.key | tojson): \(.value | tojson)") | join(",\n")),
    "}}"' > assets/reflection-defaults.json
