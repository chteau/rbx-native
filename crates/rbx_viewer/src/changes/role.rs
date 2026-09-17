//! What each instance is to the picture — see [`Role`] — and the memory of
//! it that outlives the instance, see [`Roles`].

use std::collections::HashMap;

use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::scene::{descendants, EffectKind};

/// What one instance is to the picture: which of the renderer's passes an
/// edit to it has to reach. Decided by class alone, so it can be decided
/// for an instance that has already left the DOM (see [`Roles`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    /// A `BasePart` that draws: its box or mesh, its shadow caster, its
    /// selection outline, and everything projected onto or hung off it.
    Part,
    /// A `Decal`/`Texture`, projected onto its parent part's own mesh.
    Face,
    /// A `PointLight`/`SpotLight`/`SurfaceLight`: one entry of the local
    /// light buffer, placed from its parent part.
    Light,
    /// `Lighting` itself, an `Atmosphere`, `Clouds` or a `PostEffect`: the
    /// per-frame lighting uniform and the post chain's settings.
    Lighting,
    /// The `Sky` — see [`Rebuild::Sky`].
    Sky,
    /// A `ParticleEmitter`, `Beam` or `Trail`: one of the three effect
    /// lists, re-planned whole (see `scene::effects`).
    Effect(EffectKind),
    /// An `Attachment`: the endpoint of any number of beams and trails, and
    /// where a light parented to it stands.
    Attachment,
    /// A `SpecialMesh`/`BlockMesh`/`CylinderMesh`: the shape or the file
    /// mesh its parent part draws as.
    MeshChild,
    /// A `SurfaceAppearance`: the map set its parent `MeshPart` is skinned
    /// with.
    Appearance,
    /// A `ScreenGui`/`BillboardGui`/`SurfaceGui` or anything inside one.
    Gui,
    /// A `MaterialVariant` or `MaterialService` — see [`Rebuild::Materials`].
    Material,
    /// Nothing the renderer draws from: a `Script`, a `Model`, a `Folder`, a
    /// value object, `Terrain` (never drawn, see `scene::EXCLUDED_CLASS`).
    /// A container is still walked when it moves — its subtree is what
    /// moved — but the container itself has no picture to update.
    Inert,
}

impl Role {
    pub(crate) fn of(database: &ReflectionDatabase, class: &str) -> Self {
        let is = |ancestor: &str| database.is_subclass_of(class, ancestor);
        if class == crate::scene::EXCLUDED_CLASS {
            return Role::Inert;
        }
        if is("BasePart") {
            return Role::Part;
        }
        if is("Decal") {
            return Role::Face;
        }
        if is("Light") {
            return Role::Light;
        }
        if is("Sky") {
            return Role::Sky;
        }
        if ["Lighting", "Atmosphere", "Clouds", "PostEffect"]
            .iter()
            .any(|ancestor| is(ancestor))
        {
            return Role::Lighting;
        }
        if let Some(kind) = EffectKind::of(database, class) {
            return Role::Effect(kind);
        }
        if is("Attachment") {
            return Role::Attachment;
        }
        if is("DataModelMesh") {
            return Role::MeshChild;
        }
        if is("SurfaceAppearance") {
            return Role::Appearance;
        }
        // The styling family (`StyleBase` covers `StyleSheet`/`StyleRule`;
        // `StyleDerive`/`StyleLink` hang off `Instance` directly) never
        // draws, but every GUI tree is planned through it — see
        // `scene::gui::style` — so an edit to a rule has to reach the same
        // pass an edit to the styled `Frame` would. A sheet lives outside
        // any `ScreenGui`, so `gui_changed`'s walk up from it finds no
        // canvas and replans them all, which is what a sheet's reach
        // demands.
        if is("GuiBase") || is("UIBase") || is("StyleBase") || is("StyleDerive") || is("StyleLink")
        {
            return Role::Gui;
        }
        if is("MaterialVariant") || is("MaterialService") {
            return Role::Material;
        }
        Role::Inert
    }
}

/// One instance the scene was built from, as remembered for the moment it
/// leaves the DOM: a `Change::Removed` carries nothing but a referent, and
/// by the time it is applied the instance's class and parent are gone with
/// it — yet which pass to take it out of, and which part to redraw without
/// it, depend on exactly those two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Known {
    pub(crate) role: Role,
    pub(crate) parent: Option<Ref>,
}

/// Every instance's [`Known`] as of the DOM the scene was last built or
/// patched from — kept in step by `Headless::apply_changes`, rebuilt by a
/// full reload.
#[derive(Debug, Default)]
pub(crate) struct Roles {
    known: HashMap<Ref, Known>,
}

impl Roles {
    /// Classifies every instance of `dom`. One reflection walk per distinct
    /// class rather than per instance: a place has tens of classes and tens
    /// of thousands of instances, and `is_subclass_of` is a chain of hash
    /// lookups each time.
    pub(crate) fn of_dom(dom: &WeakDom, database: &ReflectionDatabase) -> Self {
        let mut by_class: HashMap<String, Role> = HashMap::new();
        let mut known = HashMap::new();
        for referent in descendants(dom) {
            let Some(instance) = dom.get(referent) else {
                continue;
            };
            let role = *by_class
                .entry(instance.class().to_string())
                .or_insert_with(|| Role::of(database, instance.class()));
            known.insert(
                referent,
                Known {
                    role,
                    parent: dom.parent(referent),
                },
            );
        }
        Roles { known }
    }

    pub(crate) fn insert(&mut self, referent: Ref, known: Known) {
        self.known.insert(referent, known);
    }

    pub(crate) fn remove(&mut self, referent: Ref) -> Option<Known> {
        self.known.remove(&referent)
    }

    #[cfg(test)]
    pub(crate) fn get(&self, referent: Ref) -> Option<Known> {
        self.known.get(&referent).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_class_the_renderer_draws_from_has_its_role() {
        let database = ReflectionDatabase::embedded();
        for (class, role) in [
            ("Part", Role::Part),
            ("MeshPart", Role::Part),
            ("UnionOperation", Role::Part),
            ("TrussPart", Role::Part),
            ("Decal", Role::Face),
            ("Texture", Role::Face),
            ("PointLight", Role::Light),
            ("SpotLight", Role::Light),
            ("SurfaceLight", Role::Light),
            ("Lighting", Role::Lighting),
            ("Atmosphere", Role::Lighting),
            ("Clouds", Role::Lighting),
            ("BloomEffect", Role::Lighting),
            ("Sky", Role::Sky),
            ("ParticleEmitter", Role::Effect(EffectKind::Particles)),
            ("Beam", Role::Effect(EffectKind::Beams)),
            ("Trail", Role::Effect(EffectKind::Trails)),
            ("Attachment", Role::Attachment),
            ("SpecialMesh", Role::MeshChild),
            ("BlockMesh", Role::MeshChild),
            ("SurfaceAppearance", Role::Appearance),
            ("ScreenGui", Role::Gui),
            ("SurfaceGui", Role::Gui),
            ("BillboardGui", Role::Gui),
            ("Frame", Role::Gui),
            ("ImageLabel", Role::Gui),
            ("UIListLayout", Role::Gui),
            // Nothing in the styling family draws by itself, but the GUI
            // plan is read through it, so an edit to one has to rebuild the
            // GUI the way an edit to a `Frame` does.
            ("StyleSheet", Role::Gui),
            ("StyleRule", Role::Gui),
            ("StyleDerive", Role::Gui),
            ("StyleLink", Role::Gui),
            ("MaterialVariant", Role::Material),
            ("MaterialService", Role::Material),
        ] {
            assert_eq!(Role::of(&database, class), role, "{class}");
        }
    }

    // A script's source, a model's name, a value object, the camera the
    // editor writes its own pose into: none of them reach the picture, and
    // none of them is a reason to touch the GPU.
    #[test]
    fn what_the_renderer_never_reads_is_inert() {
        let database = ReflectionDatabase::embedded();
        for class in [
            "Script",
            "LocalScript",
            "ModuleScript",
            "Model",
            "Folder",
            "Workspace",
            "Camera",
            "StringValue",
            "Humanoid",
            "Sound",
            "Terrain",
        ] {
            assert_eq!(Role::of(&database, class), Role::Inert, "{class}");
        }
    }

    #[test]
    fn roles_of_a_dom_remember_each_instances_class_and_parent() {
        let database = ReflectionDatabase::embedded();
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        let part = dom.new_instance("Part", "Part", Some(workspace));
        let decal = dom.new_instance("Decal", "Decal", Some(part));

        let roles = Roles::of_dom(&dom, &database);

        assert_eq!(
            roles.get(decal),
            Some(Known {
                role: Role::Face,
                parent: Some(part),
            })
        );
        assert_eq!(
            roles.get(workspace),
            Some(Known {
                role: Role::Inert,
                parent: None,
            })
        );
        assert_eq!(roles.get(Ref::new(999)), None);
    }
}
