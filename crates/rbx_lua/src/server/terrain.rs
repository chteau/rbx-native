//! `Terrain`'s methods on a server: the voxel edits and reads go to
//! [`Physics::terrain`], the cell conversions are answered here.

use std::rc::Rc;

use mlua::{AnyUserData, IntoLua, Lua, Result, Table, Value};
use rbx_dom::CFrameData;

use super::Physics;
use crate::ctx::Ctx;
use crate::datatypes::cframe::LuaCFrame;
use crate::datatypes::color3::LuaColor3;
use crate::datatypes::int16::{LuaRegion3int16, LuaVector3int16};
use crate::datatypes::params::LuaRegion3;
use crate::datatypes::vector3::LuaVector3;
use crate::lua_enum::LuaEnumItem;
use crate::plugin::api::Arg;

/// Studs along a voxel edge.
const CELL: f64 = 4.0;

/// A voxel edit or read, in studs; materials by `Enum.Material` name.
#[derive(Debug, Clone, PartialEq)]
pub enum TerrainOp {
    FillBlock {
        cframe: CFrameData,
        size: [f64; 3],
        material: String,
    },
    FillBall {
        center: [f64; 3],
        radius: f64,
        material: String,
    },
    FillCylinder {
        cframe: CFrameData,
        height: f64,
        radius: f64,
        material: String,
    },
    FillWedge {
        cframe: CFrameData,
        size: [f64; 3],
        material: String,
    },
    /// The cells from `min` (inclusive) to `max` (exclusive).
    FillRegion {
        min: [i32; 3],
        max: [i32; 3],
        material: String,
    },
    ReplaceMaterial {
        min: [i32; 3],
        max: [i32; 3],
        from: String,
        to: String,
    },
    Read {
        min: [i32; 3],
        max: [i32; 3],
    },
    /// Cells X fastest, then Y, then Z, `size` of them from `min`.
    Write {
        min: [i32; 3],
        size: [usize; 3],
        materials: Vec<String>,
        occupancies: Vec<f64>,
    },
    Clear,
    GetColor(String),
    SetColor(String, [f32; 3]),
    /// `CountCells`: the cells holding anything.
    Count,
    /// `ReadVoxelChannels`: each cell's solid material and fill and its
    /// water, from `min` (inclusive) to `max` (exclusive).
    ReadChannels {
        min: [i32; 3],
        max: [i32; 3],
    },
    /// `WriteVoxelChannels` and `PasteRegion`: cells X fastest, `size` of
    /// them from `min`; a channel left out keeps each cell's own, and a
    /// cell whose `keep` is set is left alone.
    WriteChannels {
        min: [i32; 3],
        size: [usize; 3],
        materials: Option<Vec<String>>,
        solid: Option<Vec<f64>>,
        liquid: Option<Vec<f64>>,
        keep: Option<Vec<bool>>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum TerrainReply {
    Done,
    /// Cells X fastest, then Y, then Z.
    Voxels {
        size: [usize; 3],
        materials: Vec<String>,
        occupancies: Vec<f64>,
    },
    Color([f32; 3]),
    Count(usize),
    /// Cells X fastest, then Y, then Z.
    Channels {
        size: [usize; 3],
        materials: Vec<String>,
        solid: Vec<f64>,
        liquid: Vec<f64>,
    },
}

/// The channels `ReadVoxelChannels`/`WriteVoxelChannels` know.
const CHANNELS: [&str; 3] = ["SolidMaterial", "SolidOccupancy", "LiquidOccupancy"];

/// Roblox's terrain bounds, `Terrain.MaxExtents`, in cells.
const MAX_CELLS: i16 = 32000;

/// A `[x][y][z]` array of `size`, from cells X fastest.
fn grid_table(
    lua: &Lua,
    size: [usize; 3],
    value: impl Fn(usize) -> Result<Value>,
) -> Result<Table> {
    let out = lua.create_table()?;
    for x in 0..size[0] {
        let tx = lua.create_table()?;
        for y in 0..size[1] {
            let ty = lua.create_table()?;
            for z in 0..size[2] {
                ty.raw_set(z + 1, value(x + size[0] * (y + size[1] * z))?)?;
            }
            tx.raw_set(y + 1, ty)?;
        }
        out.raw_set(x + 1, tx)?;
    }
    Ok(out)
}

/// A `[x][y][z]` array flattened X fastest, and its size.
fn flat<T>(
    table: &Table,
    mut read: impl FnMut(Value) -> Result<T>,
) -> Result<([usize; 3], Vec<T>)> {
    let nx = table.raw_len();
    let first: Option<Table> = table.raw_get(1)?;
    let ny = first.as_ref().map_or(0, |t| t.raw_len());
    let nz = match first
        .as_ref()
        .map(|t| t.raw_get::<Option<Table>>(1))
        .transpose()?
    {
        Some(Some(t)) => t.raw_len(),
        _ => 0,
    };
    let mut cells = Vec::with_capacity(nx * ny * nz);
    for z in 0..nz {
        for y in 0..ny {
            for x in 0..nx {
                let column: Table = table.raw_get::<Table>(x + 1)?.raw_get(y + 1)?;
                cells.push(read(column.raw_get(z + 1)?)?);
            }
        }
    }
    Ok(([nx, ny, nz], cells))
}

fn material_item(lua: &Lua, ctx: &Ctx, name: &str) -> Result<Value> {
    crate::plugin::arg::to_lua(
        lua,
        ctx,
        &Arg::Enum {
            enum_name: "Material".into(),
            item: name.to_owned(),
        },
    )
}

/// What `CopyRegion` kept, by `TerrainRegion` referent.
const REGIONS: &str = "rbxn.server.terrain_regions";

fn array(v: rbx_dom::Vector3Data) -> [f64; 3] {
    [v.x, v.y, v.z].map(f64::from)
}

fn material(value: &AnyUserData) -> Result<String> {
    let item = value.borrow::<LuaEnumItem>()?;
    if item.enum_name() != "Material" {
        return Err(mlua::Error::runtime("Material expected"));
    }
    Ok(item.item_name().to_owned())
}

/// The cells a region covers, checking the resolution and the grid.
fn cells(region: &LuaRegion3, resolution: f64) -> Result<([i32; 3], [i32; 3])> {
    if resolution != CELL {
        return Err(mlua::Error::runtime("Resolution must be 4"));
    }
    let (min, max) = region.bounds();
    let (min, max) = (array(min), array(max));
    let aligned = min.iter().chain(&max).all(|v| (v / CELL).fract() == 0.0);
    if !aligned {
        return Err(mlua::Error::runtime(
            "Region has to be aligned to the grid (use Region3:ExpandToGrid)",
        ));
    }
    Ok((
        min.map(|v| (v / CELL) as i32),
        max.map(|v| (v / CELL) as i32),
    ))
}

fn vector(lua: &Lua, v: [f64; 3]) -> Result<Value> {
    LuaVector3::new(v[0] as f32, v[1] as f32, v[2] as f32).into_lua(lua)
}

/// Whether cell `c` holds anything.
fn occupied(physics: &dyn Physics, c: [i32; 3]) -> Result<bool> {
    let max = c.map(|v| v + 1);
    match physics.terrain(TerrainOp::Read { min: c, max }) {
        Ok(TerrainReply::Voxels { materials, .. }) => {
            Ok(materials.first().is_some_and(|m| m != "Air"))
        }
        Ok(_) => Ok(false),
        Err(err) => Err(mlua::Error::runtime(err)),
    }
}

/// The cell holding `p`. On a cell boundary that is the upper one, unless
/// `prefer` names what to look for (`true` solid, `false` empty) and only
/// the lower one is that (TerrainFill `world_cell_*`).
fn cell_of(physics: &dyn Physics, p: [f64; 3], prefer: Option<bool>) -> Result<[f64; 3]> {
    let mut cell = p.map(|v| (v / CELL).floor() as i32);
    if let Some(want) = prefer {
        for axis in 0..3 {
            if (p[axis] / CELL).fract() != 0.0 {
                continue;
            }
            let mut lower = cell;
            lower[axis] -= 1;
            if occupied(physics, lower)? == want && occupied(physics, cell)? != want {
                cell = lower;
            }
        }
    }
    Ok(cell.map(f64::from))
}

/// `ReadVoxels`' two arrays, indexed `[x][y][z]`, each with a `Size`.
fn voxel_tables(
    lua: &Lua,
    ctx: &Ctx,
    size: [usize; 3],
    materials: &[String],
    occupancies: &[f64],
) -> Result<(Table, Table)> {
    let mats = lua.create_table()?;
    let occs = lua.create_table()?;
    for x in 0..size[0] {
        let (mx, ox) = (lua.create_table()?, lua.create_table()?);
        for y in 0..size[1] {
            let (my, oy) = (lua.create_table()?, lua.create_table()?);
            for z in 0..size[2] {
                let i = x + size[0] * (y + size[1] * z);
                let item = Arg::Enum {
                    enum_name: "Material".into(),
                    item: materials[i].clone(),
                };
                my.raw_set(z + 1, crate::plugin::arg::to_lua(lua, ctx, &item)?)?;
                oy.raw_set(z + 1, occupancies[i])?;
            }
            mx.raw_set(y + 1, my)?;
            ox.raw_set(y + 1, oy)?;
        }
        mats.raw_set(x + 1, mx)?;
        occs.raw_set(x + 1, ox)?;
    }
    let dims = size.map(|n| n as f64);
    mats.raw_set("Size", vector(lua, dims)?)?;
    occs.raw_set("Size", vector(lua, dims)?)?;
    Ok((mats, occs))
}

/// `WriteVoxels`' arrays flattened X fastest, and their size.
fn flatten(materials: &Table, occupancies: &Table) -> Result<([usize; 3], Vec<String>, Vec<f64>)> {
    let nx = materials.raw_len();
    let first: Option<Table> = materials.raw_get(1)?;
    let ny = first.as_ref().map_or(0, |t| t.raw_len());
    let nz = match first
        .as_ref()
        .map(|t| t.raw_get::<Option<Table>>(1))
        .transpose()?
    {
        Some(Some(t)) => t.raw_len(),
        _ => 0,
    };
    let size = [nx, ny, nz];
    let count = nx * ny * nz;
    let mut mats = vec![String::new(); count];
    let mut occs = vec![0.0; count];
    for x in 0..nx {
        let (mx, ox): (Table, Table) = (materials.raw_get(x + 1)?, occupancies.raw_get(x + 1)?);
        for y in 0..ny {
            let (my, oy): (Table, Table) = (mx.raw_get(y + 1)?, ox.raw_get(y + 1)?);
            for z in 0..nz {
                let i = x + nx * (y + ny * z);
                mats[i] = material(&my.raw_get::<AnyUserData>(z + 1)?)?;
                occs[i] = oy.raw_get::<f64>(z + 1)?;
            }
        }
    }
    Ok((size, mats, occs))
}

pub(super) fn member(
    lua: &Lua,
    ctx: &Ctx,
    physics: &Rc<dyn Physics>,
    key: &str,
) -> Result<Option<Value>> {
    let ctx = ctx.clone();
    let cells_physics = physics.clone();
    let physics = physics.clone();
    let run = move |op: TerrainOp| physics.terrain(op).map_err(mlua::Error::runtime);
    if key == "MaxExtents" {
        return LuaRegion3int16 {
            min: [-MAX_CELLS; 3],
            max: [MAX_CELLS; 3],
        }
        .into_lua(lua)
        .map(Some);
    }
    let function = match key {
        "FillBlock" => lua.create_function(
            move |_, (_, cframe, size, m): (Value, LuaCFrame, LuaVector3, AnyUserData)| {
                run(TerrainOp::FillBlock {
                    cframe: cframe.0,
                    size: array(size.0),
                    material: material(&m)?,
                })
                .map(drop)
            },
        )?,
        "FillBall" => lua.create_function(
            move |_, (_, center, radius, m): (Value, LuaVector3, f64, AnyUserData)| {
                run(TerrainOp::FillBall {
                    center: array(center.0),
                    radius,
                    material: material(&m)?,
                })
                .map(drop)
            },
        )?,
        "FillCylinder" => lua.create_function(
            move |_, (_, cframe, height, radius, m): (Value, LuaCFrame, f64, f64, AnyUserData)| {
                run(TerrainOp::FillCylinder {
                    cframe: cframe.0,
                    height,
                    radius,
                    material: material(&m)?,
                })
                .map(drop)
            },
        )?,
        "FillWedge" => lua.create_function(
            move |_, (_, cframe, size, m): (Value, LuaCFrame, LuaVector3, AnyUserData)| {
                run(TerrainOp::FillWedge {
                    cframe: cframe.0,
                    size: array(size.0),
                    material: material(&m)?,
                })
                .map(drop)
            },
        )?,
        "FillRegion" => lua.create_function(
            move |_, (_, region, resolution, m): (Value, LuaRegion3, f64, AnyUserData)| {
                let (min, max) = cells(&region, resolution)?;
                run(TerrainOp::FillRegion {
                    min,
                    max,
                    material: material(&m)?,
                })
                .map(drop)
            },
        )?,
        "ReplaceMaterial" => lua.create_function(
            move |_,
                  (_, region, resolution, from, to): (
                Value,
                LuaRegion3,
                f64,
                AnyUserData,
                AnyUserData,
            )| {
                let (min, max) = cells(&region, resolution)?;
                run(TerrainOp::ReplaceMaterial {
                    min,
                    max,
                    from: material(&from)?,
                    to: material(&to)?,
                })
                .map(drop)
            },
        )?,
        "ReadVoxels" => lua.create_function(
            move |lua, (_, region, resolution): (Value, LuaRegion3, f64)| {
                let (min, max) = cells(&region, resolution)?;
                match run(TerrainOp::Read { min, max })? {
                    TerrainReply::Voxels {
                        size,
                        materials,
                        occupancies,
                    } => voxel_tables(lua, &ctx, size, &materials, &occupancies),
                    _ => Err(mlua::Error::runtime("ReadVoxels failed")),
                }
            },
        )?,
        "WriteVoxels" => lua.create_function(
            move |_,
                  (_, region, resolution, materials, occupancies): (
                Value,
                LuaRegion3,
                f64,
                Table,
                Table,
            )| {
                let (min, max) = cells(&region, resolution)?;
                let (size, materials, occupancies) = flatten(&materials, &occupancies)?;
                let wanted = [0, 1, 2].map(|i| (max[i] - min[i]).max(0) as usize);
                if size != wanted {
                    return Err(mlua::Error::runtime(
                        "Region size does not match the size of the material and occupancy arrays",
                    ));
                }
                run(TerrainOp::Write {
                    min,
                    size,
                    materials,
                    occupancies,
                })
                .map(drop)
            },
        )?,
        "Clear" => lua.create_function(move |_, _: Value| run(TerrainOp::Clear).map(drop))?,
        "CountCells" => lua.create_function(move |_, _: Value| match run(TerrainOp::Count)? {
            TerrainReply::Count(n) => Ok(n),
            _ => Err(mlua::Error::runtime("CountCells failed")),
        })?,
        "ReadVoxelChannels" => lua.create_function(
            move |lua, (_, region, resolution, wanted): (Value, LuaRegion3, f64, Table)| {
                let wanted: Vec<String> = wanted.sequence_values().collect::<Result<_>>()?;
                if let Some(bad) = wanted.iter().find(|w| !CHANNELS.contains(&w.as_str())) {
                    return Err(mlua::Error::runtime(format!("Unknown channel id '{bad}'")));
                }
                let (min, max) = cells(&region, resolution)?;
                let TerrainReply::Channels {
                    size,
                    materials,
                    solid,
                    liquid,
                } = run(TerrainOp::ReadChannels { min, max })?
                else {
                    return Err(mlua::Error::runtime("ReadVoxelChannels failed"));
                };
                let out = lua.create_table()?;
                for name in wanted {
                    let table = match name.as_str() {
                        "SolidMaterial" => {
                            grid_table(lua, size, |i| material_item(lua, &ctx, &materials[i]))?
                        }
                        "SolidOccupancy" => grid_table(lua, size, |i| Ok(Value::Number(solid[i])))?,
                        _ => grid_table(lua, size, |i| Ok(Value::Number(liquid[i])))?,
                    };
                    out.raw_set(name, table)?;
                }
                out.raw_set("Size", vector(lua, size.map(|n| n as f64))?)?;
                Ok(out)
            },
        )?,
        "WriteVoxelChannels" => lua.create_function(
            move |_, (_, region, resolution, channels): (Value, LuaRegion3, f64, Table)| {
                let (min, max) = cells(&region, resolution)?;
                let wanted = [0, 1, 2].map(|i| (max[i] - min[i]).max(0) as usize);
                let mut materials = None;
                let mut solid = None;
                let mut liquid = None;
                for pair in channels.pairs::<String, Table>() {
                    let (name, table) = pair?;
                    let size = match name.as_str() {
                        "SolidMaterial" => {
                            let (size, list) = flat(&table, |v| match v {
                                Value::UserData(u) => material(&u),
                                _ => Err(mlua::Error::runtime("Material expected")),
                            })?;
                            materials = Some(list);
                            size
                        }
                        "SolidOccupancy" | "LiquidOccupancy" => {
                            let (size, list) = flat(&table, |v| match v {
                                Value::Number(n) => Ok(n),
                                Value::Integer(n) => Ok(n as f64),
                                _ => Err(mlua::Error::runtime("number expected")),
                            })?;
                            if name == "SolidOccupancy" {
                                solid = Some(list);
                            } else {
                                liquid = Some(list);
                            }
                            size
                        }
                        _ => {
                            return Err(mlua::Error::runtime(format!(
                                "Unknown channel id '{name}'"
                            )))
                        }
                    };
                    if size != wanted {
                        return Err(mlua::Error::runtime(
                            "Region size does not match the size of the channel arrays",
                        ));
                    }
                }
                run(TerrainOp::WriteChannels {
                    min,
                    size: wanted,
                    materials,
                    solid,
                    liquid,
                    keep: None,
                })
                .map(drop)
            },
        )?,
        "CopyRegion" => lua.create_function(move |lua, (_, region): (Value, AnyUserData)| {
            let region = *region.borrow::<LuaRegion3int16>()?;
            let min = region.min.map(i32::from);
            let max = region.max.map(|v| i32::from(v) + 1);
            let TerrainReply::Channels {
                size,
                materials,
                solid,
                liquid,
            } = run(TerrainOp::ReadChannels { min, max })?
            else {
                return Err(mlua::Error::runtime("CopyRegion failed"));
            };
            let referent = ctx
                .dom_mut()
                .new_instance("TerrainRegion", "TerrainRegion", None);
            ctx.dom_mut().unparent(referent);
            let dims = size.map(|n| n as f32);
            let _ = ctx.dom_mut().set_property(
                referent,
                "SizeInCells",
                rbx_dom::Variant::Vector3(rbx_dom::Vector3Data {
                    x: dims[0],
                    y: dims[1],
                    z: dims[2],
                }),
            );
            let kept = lua.create_table()?;
            kept.raw_set("size", size.to_vec())?;
            kept.raw_set("materials", materials)?;
            kept.raw_set("solid", solid)?;
            kept.raw_set("liquid", liquid)?;
            crate::engine::events::registry_table(lua, REGIONS)?.raw_set(referent.value(), kept)?;
            crate::instance::LuaInstance::new(referent, ctx.clone()).into_lua(lua)
        })?,
        "PasteRegion" => lua.create_function(
            move |lua, (_, region, corner, empty): (Value, AnyUserData, AnyUserData, bool)| {
                let referent = region.borrow::<crate::instance::LuaInstance>()?.referent();
                let corner = corner.borrow::<LuaVector3int16>()?.0;
                let kept: Option<Table> = crate::engine::events::registry_table(lua, REGIONS)?
                    .raw_get(referent.value())?;
                let Some(kept) = kept else {
                    return Ok(());
                };
                let size: Vec<usize> = kept.raw_get("size")?;
                let materials: Vec<String> = kept.raw_get("materials")?;
                let keep = (!empty).then(|| materials.iter().map(|m| m == "Air").collect());
                run(TerrainOp::WriteChannels {
                    min: corner.map(i32::from),
                    size: [size[0], size[1], size[2]],
                    materials: Some(materials),
                    solid: Some(kept.raw_get("solid")?),
                    liquid: Some(kept.raw_get("liquid")?),
                    keep,
                })
                .map(drop)
            },
        )?,
        "GetMaterialColor" => {
            lua.create_function(move |_, (_, m): (Value, AnyUserData)| {
                match run(TerrainOp::GetColor(material(&m)?))? {
                    TerrainReply::Color([r, g, b]) => Ok(LuaColor3::new(r, g, b)),
                    _ => Err(mlua::Error::runtime("GetMaterialColor failed")),
                }
            })?
        }
        "SetMaterialColor" => {
            lua.create_function(move |_, (_, m, color): (Value, AnyUserData, LuaColor3)| {
                let c = color.0;
                run(TerrainOp::SetColor(material(&m)?, [c.r, c.g, c.b])).map(drop)
            })?
        }
        "CellCenterToWorld" => {
            lua.create_function(|lua, (_, x, y, z): (Value, f64, f64, f64)| {
                vector(lua, [x, y, z].map(|c| c * CELL + CELL / 2.0))
            })?
        }
        "CellCornerToWorld" => {
            lua.create_function(|lua, (_, x, y, z): (Value, f64, f64, f64)| {
                vector(lua, [x, y, z].map(|c| c * CELL))
            })?
        }
        "WorldToCell" | "WorldToCellPreferSolid" | "WorldToCellPreferEmpty" => {
            let prefer = match key {
                "WorldToCellPreferSolid" => Some(true),
                "WorldToCellPreferEmpty" => Some(false),
                _ => None,
            };
            lua.create_function(move |lua, (_, p): (Value, LuaVector3)| {
                vector(lua, cell_of(&*cells_physics, array(p.0), prefer)?)
            })?
        }
        _ => return Ok(None),
    };
    Ok(Some(Value::Function(function)))
}
