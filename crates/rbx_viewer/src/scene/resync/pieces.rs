//! The union arm of [`Scene::resync_part`]: what a legacy
//! `UnionOperation`/`NegateOperation` draws as after an edit to it.
//!
//! A union is the one instance a place draws as *several* boxes — the
//! additive pieces recovered from its operation tree, when the boolean over
//! that tree could not be computed (see `scene::union`). Each piece is its
//! own instance in every box pass, told apart by its own
//! [`PartId`](super::PartId), so moving, recolouring, hiding or deleting the
//! union rewrites those records in place instead of rebuilding the scene.
//!
//! The boolean is never re-run here. It is a function of the asset's bytes
//! alone, so `load::Resident` keeps what it carved and this re-reads it: an
//! edit to a union costs the tree walk that places its half-dozen pieces,
//! not a BSP build.

use rbx_dom::Ref;

use crate::changes::Rebuild;
use crate::scene::{union, Part, Scene, UnionEvaluations};

use super::Drawn;

impl Scene {
    /// What `referent`'s re-planned union draws as, with `part` its own
    /// freshly built box — suppressed here if the union's asset carved to
    /// anything at all, exactly as `Scene::resolve_unions` suppresses it.
    ///
    /// The pieces are written into the scene as they are derived; the caller
    /// is what puts `part` itself back and what tells the renderer which
    /// piece slots are no longer filled.
    pub(super) fn resync_union(
        &mut self,
        part: &mut Part,
        entry: union::Entry,
        unions: &UnionEvaluations,
        known_layers: usize,
    ) -> Result<Drawn, Rebuild> {
        let referent = part.referent();
        let Some(evaluated) = unions.of(entry.asset()) else {
            // Nothing carved for the asset yet, whether nobody has fetched
            // it or the boolean over it is still pending: the same "not
            // landed yet" case as a file mesh's asset not resolving, so the
            // box until it does — never a rebuild for this reason, since the
            // asset is asked for in the background (see
            // `Headless::apply_changes`) and folded in whenever it lands.
            self.resolved_file_meshes.remove(referent);
            return Ok(Drawn::Box(*part));
        };

        // Carved either way, so the union's own box hides behind the result
        // — the pieces stand inside it, and a computed mesh replaces it.
        part.suppressed = true;
        if !evaluated.is_carved() {
            let pieces = entry.pieces(evaluated, &self.database, &mut self.materials);
            // A leaf's own `Material` can be one the catalog only learned
            // while this union was resolved; a layer past what the renderer
            // uploaded maps to nothing it can shade.
            if pieces
                .iter()
                .any(|piece| piece.material.layer as usize >= known_layers)
            {
                return Err(Rebuild::Asset);
            }
            self.resolved_file_meshes.remove(referent);
            self.place_pieces(referent, &pieces);
            return Ok(Drawn::Pieces {
                placement: part.placement(),
                pieces,
            });
        }

        // Fully transparent: a fresh resolution lists no instance for it,
        // and the box above stays hidden so it cannot reappear underneath.
        if entry.is_invisible() {
            self.resolved_file_meshes.remove(referent);
            return Ok(Drawn::Gone);
        }
        match entry.patched(evaluated, &self.resolved_file_meshes) {
            // A texture, `SurfaceAppearance` set or material sample not
            // finished downloading: the same "not landed yet" case as the
            // mesh itself not resolving, so the box until it does.
            None => {
                part.suppressed = false;
                self.resolved_file_meshes.remove(referent);
                Ok(Drawn::Box(*part))
            }
            Some(instance) if instance.material.layer as usize >= known_layers => {
                Err(Rebuild::Asset)
            }
            Some(instance) => Ok(Drawn::Mesh(self.place_instance(instance))),
        }
    }

    /// Writes `pieces` into the scene, each over the piece it re-derives
    /// where the scene already drew one — the whole point of numbering them
    /// — and appending the rest.
    fn place_pieces(&mut self, referent: Ref, pieces: &[Part]) {
        let mut placed = vec![false; pieces.len()];
        for held in self.parts.iter_mut() {
            if held.referent() != referent {
                continue;
            }
            let Some(index) = held.id.piece_index().map(|index| index as usize) else {
                continue;
            };
            if let Some(piece) = pieces.get(index) {
                *held = *piece;
                placed[index] = true;
            }
        }
        for piece in pieces
            .iter()
            .zip(&placed)
            .filter(|(_, placed)| !**placed)
            .map(|(piece, _)| *piece)
        {
            self.push_part(piece);
        }
    }
}
