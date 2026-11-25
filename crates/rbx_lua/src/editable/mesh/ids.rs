//! `EditableMesh` ids and the pools that hand them out.
//!
//! An id is a number with three fields, read off the ids real Roblox hands
//! out (the EditableMeshTopology golden): the element's slot in the low 32
//! bits, its kind in the next three (1 vertex, 2 normal, 3 color, 4 UV,
//! 5 face, 6 bone) and how many times the slot was reused above that.
//! `IdDebugString` prints them back as `v5` or, for a reused slot, `v5,2`.
//!
//! Each kind keeps its elements in one packed list, newest last: removing
//! one moves the last into its place, and a freed slot is the next one
//! handed out (the last freed first). That is the order `GetVertices` and
//! friends report.

use mlua::Result;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Kind {
    Vertex = 1,
    Normal = 2,
    Color = 3,
    Uv = 4,
    Face = 5,
    Bone = 6,
}

impl Kind {
    fn from_tag(tag: u64) -> Option<Kind> {
        Some(match tag {
            1 => Kind::Vertex,
            2 => Kind::Normal,
            3 => Kind::Color,
            4 => Kind::Uv,
            5 => Kind::Face,
            6 => Kind::Bone,
            _ => return None,
        })
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Kind::Vertex => "Vertex",
            Kind::Normal => "Normal",
            Kind::Color => "Color",
            Kind::Uv => "UV",
            Kind::Face => "Face",
            Kind::Bone => "Bone",
        }
    }

    fn letter(self) -> char {
        match self {
            Kind::Vertex => 'v',
            Kind::Normal => 'n',
            Kind::Color => 'c',
            Kind::Uv => 'u',
            Kind::Face => 'f',
            Kind::Bone => 'b',
        }
    }
}

pub(crate) type Id = u64;

const SLOT_BITS: u32 = 32;
const VERSION_SHIFT: u32 = 35;

pub(crate) fn encode(kind: Kind, slot: u32, version: u32) -> Id {
    (u64::from(version - 1) << VERSION_SHIFT) | ((kind as u64) << SLOT_BITS) | u64::from(slot)
}

/// The kind, slot and version a script's number names, if it is shaped
/// like an id at all.
pub(crate) fn decode(value: f64) -> Option<(Kind, u32, u32)> {
    if !(value >= 0.0 && value.fract() == 0.0 && value < 9.0e15) {
        return None;
    }
    let id = value as u64;
    let kind = Kind::from_tag((id >> SLOT_BITS) & 7)?;
    let version = (id >> VERSION_SHIFT) as u32 + 1;
    Some((kind, id as u32, version))
}

pub(crate) fn debug_string(value: f64) -> String {
    match decode(value) {
        Some((kind, slot, 1)) => format!("{}{slot}", kind.letter()),
        Some((kind, slot, version)) => format!("{}{slot},{version}", kind.letter()),
        None => format!("?{value}"),
    }
}

pub(crate) fn kind_error(expected: Kind, value: f64) -> mlua::Error {
    let received = match decode(value) {
        Some((kind, ..)) => format!("{} id", kind.name()),
        None => "value that doesn't appear to be an id".to_owned(),
    };
    mlua::Error::runtime(format!(
        "Unexpected EditableMesh id type. Expected {} id, received {received}",
        expected.name()
    ))
}

#[derive(Clone)]
pub(crate) struct Pool<T> {
    kind: Kind,
    /// (slot, value), newest last.
    items: Vec<(u32, T)>,
    /// Per slot (index = slot - 1): where it sits in `items`, its version.
    at: Vec<Option<usize>>,
    versions: Vec<u32>,
    free: Vec<u32>,
}

impl<T> Pool<T> {
    pub(crate) fn new(kind: Kind) -> Pool<T> {
        Pool {
            kind,
            items: Vec::new(),
            at: Vec::new(),
            versions: Vec::new(),
            free: Vec::new(),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.items.len()
    }

    /// The next id [`insert`](Self::insert) will hand out.
    pub(crate) fn reserve(&mut self) -> Id {
        let slot = match self.free.pop() {
            Some(slot) => {
                self.versions[slot as usize - 1] += 1;
                slot
            }
            None => {
                self.at.push(None);
                self.versions.push(1);
                self.at.len() as u32
            }
        };
        encode(self.kind, slot, self.versions[slot as usize - 1])
    }

    /// Puts `value` under an id from [`reserve`](Self::reserve).
    pub(crate) fn place(&mut self, id: Id, value: T) {
        let slot = id as u32;
        self.at[slot as usize - 1] = Some(self.items.len());
        self.items.push((slot, value));
    }

    pub(crate) fn insert(&mut self, value: T) -> Id {
        let id = self.reserve();
        self.place(id, value);
        id
    }

    /// Hands a reserved id's slot back unused.
    pub(crate) fn release(&mut self, id: Id) {
        self.free.push(id as u32);
    }

    fn position(&self, id: Id) -> Option<usize> {
        let slot = id as u32;
        if (id >> SLOT_BITS) & 7 != self.kind as u64 {
            return None;
        }
        let index = (slot as usize).checked_sub(1)?;
        let version = (id >> VERSION_SHIFT) as u32 + 1;
        (self.versions.get(index) == Some(&version))
            .then(|| self.at[index])
            .flatten()
    }

    pub(crate) fn contains(&self, id: Id) -> bool {
        self.position(id).is_some()
    }

    pub(crate) fn get(&self, id: Id) -> Option<&T> {
        self.position(id).map(|i| &self.items[i].1)
    }

    pub(crate) fn get_mut(&mut self, id: Id) -> Option<&mut T> {
        self.position(id).map(|i| &mut self.items[i].1)
    }

    /// The live element in `slot`, whatever its version: how `AddFace`
    /// reads its vertex ids.
    pub(crate) fn by_slot(&self, slot: u32) -> Option<Id> {
        let index = (slot as usize).checked_sub(1)?;
        self.at.get(index).copied().flatten()?;
        Some(encode(self.kind, slot, self.versions[index]))
    }

    pub(crate) fn remove(&mut self, id: Id) -> Option<T> {
        let i = self.position(id)?;
        let (slot, value) = self.items.swap_remove(i);
        self.at[slot as usize - 1] = None;
        if let Some(&(moved, _)) = self.items.get(i) {
            self.at[moved as usize - 1] = Some(i);
        }
        self.free.push(slot);
        Some(value)
    }

    fn id_at(&self, i: usize) -> Id {
        let slot = self.items[i].0;
        encode(self.kind, slot, self.versions[slot as usize - 1])
    }

    pub(crate) fn ids(&self) -> Vec<Id> {
        (0..self.items.len()).map(|i| self.id_at(i)).collect()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (Id, &T)> + '_ {
        (0..self.items.len()).map(move |i| (self.id_at(i), &self.items[i].1))
    }

    pub(crate) fn values_mut(&mut self) -> impl Iterator<Item = &mut T> + '_ {
        self.items.iter_mut().map(|(_, value)| value)
    }

    /// The id `value` names if it is a live element of this pool, or the
    /// error Roblox gives: the wrong kind, or one that is gone.
    pub(crate) fn check(&self, value: f64) -> Result<Id> {
        match decode(value) {
            Some((kind, ..)) if kind == self.kind => {
                let id = value as u64;
                match self.contains(id) {
                    true => Ok(id),
                    false => Err(mlua::Error::runtime("Invalid id")),
                }
            }
            _ => Err(kind_error(self.kind, value)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_match_the_numbers_roblox_hands_out() {
        // The golden's raw ids: v1, f1, n1, u1, c1, b1 and a reused v5.
        assert_eq!(encode(Kind::Vertex, 1, 1), 4294967297);
        assert_eq!(encode(Kind::Face, 1, 1), 21474836481);
        assert_eq!(encode(Kind::Normal, 1, 1), 8589934593);
        assert_eq!(encode(Kind::Uv, 1, 1), 17179869185);
        assert_eq!(encode(Kind::Color, 1, 1), 12884901889);
        assert_eq!(encode(Kind::Bone, 1, 1), 25769803777);
        assert_eq!(encode(Kind::Vertex, 5, 2), 38654705669);
        assert_eq!(debug_string(38654705669.0), "v5,2");
        assert_eq!(debug_string(123456.0), "?123456");
    }

    #[test]
    fn removing_moves_the_last_in_and_frees_the_slot_for_reuse() {
        let mut pool = Pool::new(Kind::Vertex);
        let ids: Vec<Id> = (0..6).map(|i| pool.insert(i)).collect();
        pool.remove(ids[4]);
        let reused = pool.insert(9);
        let names: Vec<String> = pool
            .ids()
            .iter()
            .map(|&id| debug_string(id as f64))
            .collect();
        assert_eq!(names, ["v1", "v2", "v3", "v4", "v6", "v5,2"]);
        assert!(!pool.contains(ids[4]));
        assert_eq!(pool.get(reused), Some(&9));
        assert!(pool.check(ids[4] as f64).is_err());
    }
}
