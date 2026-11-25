//! A client's editable memory budget. Roblox gives a client a fixed
//! amount for editables and hands back `nil` past it, while servers,
//! Studio and plugins have no limit. The sizes are fitted to what a phone
//! client allowed (the EditableBudget golden): twenty 1024² images,
//! seven empty growable meshes, 226 fixed copies of a 300-vertex mesh.
//! An editable no script holds any more gives its share back, as one
//! destroyed does.

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use mlua::Lua;
use rbx_dom::Ref;

use super::mesh::MeshData;

const IMAGE_BUDGET: u64 = 82 << 20;
const MESH_BUDGET: u64 = 64 << 20;
/// What a mesh that can still grow reserves.
const GROWABLE: u64 = MESH_BUDGET * 2 / 15;
/// Per vertex and per face.
const ELEMENT: u64 = 427;

#[derive(Default)]
struct Budget {
    images: u64,
    meshes: u64,
    held: HashMap<Ref, (bool, u64, Weak<()>)>,
}

impl Budget {
    fn give_back(&mut self, referent: Ref) {
        if let Some((mesh, cost, _)) = self.held.remove(&referent) {
            match mesh {
                true => self.meshes -= cost,
                false => self.images -= cost,
            }
        }
    }

    /// Returns the shares of editables whose handles were all collected.
    fn prune(&mut self) {
        let gone: Vec<Ref> = self
            .held
            .iter()
            .filter(|(_, h)| h.2.strong_count() == 0)
            .map(|(r, _)| *r)
            .collect();
        for referent in gone {
            self.give_back(referent);
        }
    }

    fn fits(&self, mesh: bool, cost: u64) -> bool {
        match mesh {
            true => self.meshes + cost <= MESH_BUDGET,
            false => self.images + cost <= IMAGE_BUDGET,
        }
    }
}

pub(crate) fn mesh_cost(data: &MeshData) -> u64 {
    let elements = (data.vertices.len() + data.faces.len()) as u64 * ELEMENT;
    match data.fixed {
        true => elements,
        false => GROWABLE + elements,
    }
}

pub(crate) fn image_cost(width: u32, height: u32) -> u64 {
    u64::from(width) * u64::from(height) * 4
}

/// Whether a client has room for an editable of `cost`, after collecting
/// the ones scripts dropped if it hasn't.
pub(crate) fn admit(lua: &Lua, client: bool, mesh: bool, cost: u64) -> bool {
    if !client {
        return true;
    }
    let fits = |lua: &Lua| match lua.app_data_mut::<Budget>() {
        Some(mut budget) => {
            budget.prune();
            budget.fits(mesh, cost)
        }
        None => Budget::default().fits(mesh, cost),
    };
    fits(lua) || (lua.gc_collect().is_ok() && lua.gc_collect().is_ok() && fits(lua))
}

/// Charges a client for a new editable, held while `token` lives.
pub(crate) fn hold(lua: &Lua, client: bool, referent: Ref, mesh: bool, cost: u64, token: &Arc<()>) {
    if !client {
        return;
    }
    if lua.app_data_ref::<Budget>().is_none() {
        lua.set_app_data(Budget::default());
    }
    let mut budget = lua.app_data_mut::<Budget>().expect("just set");
    match mesh {
        true => budget.meshes += cost,
        false => budget.images += cost,
    }
    budget
        .held
        .insert(referent, (mesh, cost, Arc::downgrade(token)));
}

/// A destroyed editable's share back.
pub(crate) fn release(lua: &Lua, referent: Ref) {
    if let Some(mut budget) = lua.app_data_mut::<Budget>() {
        budget.give_back(referent);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_budget_fits_what_a_phone_client_allowed() {
        let empty = mesh_cost(&MeshData::default());
        assert_eq!(MESH_BUDGET / empty, 7);
        let fixed = 598 * ELEMENT;
        assert_eq!((MESH_BUDGET - empty - fixed) / fixed, 226);
        assert_eq!(MESH_BUDGET / (empty + fixed), 7);
        assert_eq!(IMAGE_BUDGET / image_cost(1024, 1024), 20);
    }
}
