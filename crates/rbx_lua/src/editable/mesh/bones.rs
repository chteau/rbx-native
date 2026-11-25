//! Bones, skin weights and FACS poses, with the rules the
//! EditableMeshBones golden shows: every bone needs a name, `Virtual` and
//! a `CFrame`; names are unique and at most 100 characters; a vertex has
//! up to four bones; poses take only virtual bones.

use mlua::{IntoLuaMulti, Lua, MultiValue, Result, Table, UserDataMethods, Value};
use rbx_dom::CFrameData;

use super::data::{Bone, MeshData, Pose};
use super::ids::Id;
use super::{id_arg, numbers, sequence, LuaEditableMesh};
use crate::datatypes::cframe::LuaCFrame;
use crate::lua_enum::LuaEnumItem;

const MAX_NAME: usize = 100;

fn error(message: &str) -> mlua::Error {
    mlua::Error::runtime(format!("EditableMesh Error: {message}"))
}

fn cframe(value: &Value) -> Result<CFrameData> {
    match value {
        Value::UserData(d) if d.is::<LuaCFrame>() => Ok(d.borrow::<LuaCFrame>()?.0),
        other => Err(mlua::Error::runtime(format!(
            "Unable to cast {} to CFrame",
            other.type_name()
        ))),
    }
}

fn action(value: &Value) -> Result<(u32, String)> {
    match value {
        Value::UserData(d) if d.is::<LuaEnumItem>() => {
            let item = d.borrow::<LuaEnumItem>()?;
            Ok((item.value(), item.item_name().to_owned()))
        }
        other => Err(mlua::Error::runtime(format!(
            "Unable to cast {} to Enum.FacsActionUnit",
            other.type_name()
        ))),
    }
}

/// `Enum.FacsActionUnit` items for `values`, in order.
fn actions(lua: &Lua, values: &[u32]) -> Result<Table> {
    lua.load(
        "local values = ... local items, out = Enum.FacsActionUnit:GetEnumItems(), {}
         for i, v in values do for _, item in items do if item.Value == v then out[i] = item end end end
         return out",
    )
    .call(values.to_vec())
}

fn unique_name(m: &MeshData, name: &str, except: Option<Id>) -> Result<()> {
    if name.chars().count() > MAX_NAME {
        return Err(error("Bone name too long"));
    }
    match m
        .bones
        .iter()
        .any(|(id, b)| b.name == name && Some(id) != except)
    {
        true => Err(error("Bone name already exists in the mesh")),
        false => Ok(()),
    }
}

fn add_bone(m: &mut MeshData, props: &Table) -> Result<Id> {
    for pair in props.pairs::<Value, Value>() {
        let (key, _) = pair?;
        let key = key.to_string()?;
        if !matches!(key.as_str(), "Name" | "ParentId" | "CFrame" | "Virtual") {
            return Err(mlua::Error::runtime(format!(
                "Unexpected entry in table: {key}"
            )));
        }
    }
    let missing = |what: &str| mlua::Error::runtime(format!("Missing bone {what} value"));
    let name: String = props
        .get::<Option<String>>("Name")?
        .ok_or_else(|| missing("Name"))?;
    let is_virtual: bool = props
        .get::<Option<bool>>("Virtual")?
        .ok_or_else(|| missing("Virtual"))?;
    let frame: Value = props.get("CFrame")?;
    if frame.is_nil() {
        return Err(missing("CFrame"));
    }
    let frame = cframe(&frame)?;
    let parent = match props.get::<Value>("ParentId")? {
        Value::Nil => 0,
        id => match id_arg(&id, 1)? {
            0.0 => 0,
            id => m.bones.check(id)?,
        },
    };
    unique_name(m, &name, None)?;
    Ok(m.bones.insert(Bone {
        name,
        parent,
        cframe: frame,
        is_virtual,
    }))
}

fn set_parent(m: &mut MeshData, bone: Id, parent: f64) -> Result<()> {
    let parent = match parent {
        0.0 => 0,
        p => m.bones.check(p)?,
    };
    let mut at = parent;
    while at != 0 {
        if at == bone {
            return Err(error("Invalid bone parent"));
        }
        at = m.bones.get(at).map_or(0, |b| b.parent);
    }
    m.bones.get_mut(bone).expect("checked").parent = parent;
    Ok(())
}

/// A pose's bones checked: as many frames as bones, every bone virtual.
fn pose(m: &MeshData, bones: &[f64], frames: &[Value]) -> Result<Pose> {
    if bones.len() != frames.len() {
        return Err(mlua::Error::runtime(format!(
            "boneIds and cframes size mismatch ({} and {})",
            bones.len(),
            frames.len()
        )));
    }
    bones
        .iter()
        .zip(frames)
        .map(|(&b, f)| {
            let id = m.bones.check(b)?;
            let bone = m.bones.get(id).expect("checked");
            if !bone.is_virtual {
                return Err(mlua::Error::runtime(format!(
                    "Only virtual bones can be used in Facs Poses, unable to use bone \"{}\", id {id}",
                    bone.name
                )));
            }
            Ok((id, cframe(f)?))
        })
        .collect()
}

fn corrective_key(list: &Table) -> Result<Vec<u32>> {
    let items: Vec<(u32, String)> = list
        .sequence_values::<Value>()
        .map(|v| action(&v?))
        .collect::<Result<_>>()?;
    if !matches!(items.len(), 2 | 3) {
        return Err(mlua::Error::runtime(
            "Correctives must have 2 or 3 FACS controls",
        ));
    }
    let sides = |prefix: &str| items.iter().any(|(_, n)| n.starts_with(prefix));
    if sides("Left") && sides("Right") {
        return Err(mlua::Error::runtime(
            "Unable to use actions together in a corrective",
        ));
    }
    let mut key: Vec<u32> = items.into_iter().map(|(v, _)| v).collect();
    key.sort_unstable();
    Ok(key)
}

fn pose_values(lua: &Lua, pose: Option<&Pose>) -> Result<MultiValue> {
    let pose = pose.cloned().unwrap_or_default();
    let bones = sequence(lua, pose.iter().map(|(b, _)| *b))?;
    let frames = lua.create_sequence_from(pose.into_iter().map(|(_, f)| LuaCFrame(f)))?;
    (bones, frames).into_lua_multi(lua)
}

pub(super) fn add<M: UserDataMethods<LuaEditableMesh>>(methods: &mut M) {
    methods.add_method("AddBone", |_, this, props: Table| {
        this.edit(|m| add_bone(m, &props).map(|id| id as f64))
    });
    methods.add_method("RemoveBone", |_, this, bone: Value| {
        let bone = id_arg(&bone, 1)?;
        this.edit(|m| {
            let bone = m.bones.check(bone)?;
            m.bones.remove(bone);
            m.forget_bone(bone);
            Ok(())
        })
    });
    methods.add_method("GetBones", |lua, this, ()| {
        this.read(|m| sequence(lua, m.bones.ids()))
    });
    methods.add_method("GetBoneByName", |_, this, name: String| {
        this.read(|m| {
            m.bones
                .iter()
                .find(|(_, b)| b.name == name)
                .map(|(id, _)| id as f64)
                .ok_or_else(|| mlua::Error::runtime(format!("No bone named \"{name}\"")))
        })
    });
    methods.add_method("GetBoneName", |_, this, bone: Value| {
        let bone = id_arg(&bone, 1)?;
        this.read(|m| {
            Ok(m.bones
                .get(m.bones.check(bone)?)
                .expect("checked")
                .name
                .clone())
        })
    });
    methods.add_method("SetBoneName", |_, this, (bone, name): (Value, String)| {
        let bone = id_arg(&bone, 1)?;
        this.edit(|m| {
            let bone = m.bones.check(bone)?;
            unique_name(m, &name, Some(bone))?;
            m.bones.get_mut(bone).expect("checked").name = name;
            Ok(())
        })
    });
    methods.add_method("GetBoneParent", |_, this, bone: Value| {
        let bone = id_arg(&bone, 1)?;
        this.read(|m| Ok(m.bones.get(m.bones.check(bone)?).expect("checked").parent as f64))
    });
    methods.add_method(
        "SetBoneParent",
        |_, this, (bone, parent): (Value, Value)| {
            let (bone, parent) = (id_arg(&bone, 1)?, id_arg(&parent, 2)?);
            this.edit(|m| {
                let bone = m.bones.check(bone)?;
                set_parent(m, bone, parent)
            })
        },
    );
    methods.add_method("GetBoneCFrame", |_, this, bone: Value| {
        let bone = id_arg(&bone, 1)?;
        this.read(|m| {
            Ok(LuaCFrame(
                m.bones.get(m.bones.check(bone)?).expect("checked").cframe,
            ))
        })
    });
    methods.add_method("SetBoneCFrame", |_, this, (bone, frame): (Value, Value)| {
        let (bone, frame) = (id_arg(&bone, 1)?, cframe(&frame)?);
        this.edit(|m| {
            let bone = m.bones.check(bone)?;
            m.bones.get_mut(bone).expect("checked").cframe = frame;
            Ok(())
        })
    });
    methods.add_method("GetBoneIsVirtual", |_, this, bone: Value| {
        let bone = id_arg(&bone, 1)?;
        this.read(|m| {
            Ok(m.bones
                .get(m.bones.check(bone)?)
                .expect("checked")
                .is_virtual)
        })
    });
    methods.add_method(
        "SetBoneIsVirtual",
        |_, this, (bone, is_virtual): (Value, bool)| {
            let bone = id_arg(&bone, 1)?;
            this.edit(|m| {
                let bone = m.bones.check(bone)?;
                m.bones.get_mut(bone).expect("checked").is_virtual = is_virtual;
                Ok(())
            })
        },
    );

    methods.add_method("GetVertexBones", |lua, this, v: Value| {
        let v = id_arg(&v, 1)?;
        this.read(|m| {
            sequence(
                lua,
                m.vertices
                    .get(m.vertices.check(v)?)
                    .expect("checked")
                    .bones
                    .clone(),
            )
        })
    });
    methods.add_method("SetVertexBones", |_, this, (v, bones): (Value, Table)| {
        let (v, bones) = (id_arg(&v, 1)?, numbers(&bones)?);
        this.edit(|m| {
            let v = m.vertices.check(v)?;
            let bones: Vec<Id> = bones
                .iter()
                .map(|&b| m.bones.check(b))
                .collect::<Result<_>>()?;
            if bones.len() > 4 {
                return Err(mlua::Error::runtime("Too many bone ids provided"));
            }
            let vertex = m.vertices.get_mut(v).expect("checked");
            vertex.weights.resize(bones.len(), 0);
            vertex.bones = bones;
            Ok(())
        })
    });
    methods.add_method("GetVertexBoneWeights", |lua, this, v: Value| {
        let v = id_arg(&v, 1)?;
        this.read(|m| {
            let weights = &m
                .vertices
                .get(m.vertices.check(v)?)
                .expect("checked")
                .weights;
            lua.create_sequence_from(weights.iter().map(|&w| f32::from(w) / 255.0))
        })
    });
    methods.add_method(
        "SetVertexBoneWeights",
        |_, this, (v, weights): (Value, Vec<f64>)| {
            let v = id_arg(&v, 1)?;
            this.edit(|m| {
                let v = m.vertices.check(v)?;
                let vertex = m.vertices.get_mut(v).expect("checked");
                if weights.len() != vertex.bones.len() {
                    return Err(mlua::Error::runtime(
                        "Number of bone weights provided does not match number of bone ids",
                    ));
                }
                vertex.weights = weights
                    .iter()
                    .map(|w| (w * 256.0).floor().clamp(0.0, 255.0) as u8)
                    .collect();
                Ok(())
            })
        },
    );

    methods.add_method(
        "SetFacsPose",
        |_, this, (unit, bones, frames): (Value, Table, Vec<Value>)| {
            let (unit, _) = action(&unit)?;
            let bones = numbers(&bones)?;
            this.edit(|m| {
                let pose = pose(m, &bones, &frames)?;
                match pose.is_empty() {
                    true => m.poses.remove(&unit),
                    false => m.poses.insert(unit, pose),
                };
                Ok(())
            })
        },
    );
    methods.add_method(
        "SetFacsBonePose",
        |_, this, (unit, bone, frame): (Value, Value, Value)| {
            let (unit, _) = action(&unit)?;
            let bone = id_arg(&bone, 2)?;
            this.edit(|m| {
                let (id, frame) = pose(m, &[bone], &[frame])?.remove(0);
                let entry = m.poses.entry(unit).or_default();
                match entry.iter_mut().find(|(b, _)| *b == id) {
                    Some(slot) => slot.1 = frame,
                    None => entry.push((id, frame)),
                }
                Ok(())
            })
        },
    );
    methods.add_method("GetFacsPose", |lua, this, unit: Value| {
        let (unit, _) = action(&unit)?;
        this.read(|m| pose_values(lua, m.poses.get(&unit)))
    });
    methods.add_method("GetFacsPoses", |lua, this, ()| {
        let units: Vec<u32> = this.read(|m| Ok(m.poses.keys().copied().collect()))?;
        actions(lua, &units)
    });
    methods.add_method(
        "SetFacsCorrectivePose",
        |_, this, (units, bones, frames): (Table, Table, Vec<Value>)| {
            let key = corrective_key(&units)?;
            let bones = numbers(&bones)?;
            this.edit(|m| {
                let pose = pose(m, &bones, &frames)?;
                match pose.is_empty() {
                    true => m.correctives.remove(&key),
                    false => m.correctives.insert(key, pose),
                };
                Ok(())
            })
        },
    );
    methods.add_method("GetFacsCorrectivePose", |lua, this, units: Table| {
        let key = corrective_key(&units)?;
        this.read(|m| pose_values(lua, m.correctives.get(&key)))
    });
    methods.add_method("GetFacsCorrectivePoses", |lua, this, ()| {
        let keys: Vec<Vec<u32>> = this.read(|m| Ok(m.correctives.keys().cloned().collect()))?;
        let out = lua.create_table()?;
        for (i, key) in keys.iter().enumerate() {
            out.raw_set(i + 1, actions(lua, key)?)?;
        }
        Ok(out)
    });
}
