//! Studio's own skybox: six DXT1-compressed DDS panels bundled inside
//! `content-textures3.zip`, one of the native Studio content packages
//! `rbx_assets::NativeContent` resolves `rbxasset://` paths against.
//!
//! [`super::panels_or_default`] reaches for this set whenever a place has no
//! `Sky` at all, or the one it has leaves a panel empty or unparseable — a
//! partial custom sky is worse than Roblox's own default, not better.

use rbx_assets::AssetRef;
use rbx_dom::{Ref, WeakDom};

use super::{quad, Quad, SkyFace, SKY_FACES};

impl SkyFace {
    /// The lowercase suffix Studio's own panel files use — matching the
    /// property name's own casing (`SkyboxUp` -> `up`, ...), not the GL/D3D
    /// cube map convention (`+y`, ...) some other engines use.
    fn default_suffix(self) -> &'static str {
        match self {
            SkyFace::Rt => "rt",
            SkyFace::Lf => "lf",
            SkyFace::Up => "up",
            SkyFace::Dn => "dn",
            SkyFace::Bk => "bk",
            SkyFace::Ft => "ft",
        }
    }
}

/// `rbxasset://sky/sky512_{up,dn,lf,rt,ft,bk}.tex`, on the same face basis a
/// user-supplied `Sky` uses — so a default sky seams and orients exactly like
/// a real one would.
pub(super) fn panels() -> Vec<(AssetRef, Quad)> {
    SKY_FACES
        .into_iter()
        .map(|face| {
            let path = format!("sky/sky512_{}.tex", face.default_suffix());
            (AssetRef::Native(path), quad(face))
        })
        .collect()
}

/// `sky`'s panels if fully resolvable, else Studio's own default skybox — no
/// `Sky` and a `Sky` with a bad panel fall back alike.
pub(in crate::textures) fn panels_or_default(
    dom: &WeakDom,
    sky: Option<Ref>,
) -> Vec<(AssetRef, Quad)> {
    sky.and_then(|referent| super::panels(dom, referent))
        .unwrap_or_else(panels)
}

#[cfg(test)]
mod tests {
    use rbx_dom::{Instance, Variant};

    use super::*;

    fn sky_with(properties: &[(&str, Variant)]) -> (WeakDom, Ref) {
        let mut dom = WeakDom::new();
        let referent = Ref::new(1);
        let mut sky = Instance::new(referent, "Sky", "Sky");
        for (name, value) in properties {
            sky.properties_mut()
                .insert((*name).to_string(), value.clone());
        }
        dom.insert(sky);
        dom.set_parent(referent, None);
        (dom, referent)
    }

    // The one case this whole module exists for: a place that never had a
    // `Sky` still gets Studio's own skybox instead of a blank background.
    #[test]
    fn no_sky_at_all_plans_the_default_skybox() {
        let dom = WeakDom::new();
        assert_eq!(panels_or_default(&dom, None), panels());
    }

    #[test]
    fn a_sky_with_no_resolvable_panels_falls_back_too() {
        let (dom, referent) = sky_with(&[]);
        assert_eq!(panels_or_default(&dom, Some(referent)), panels());
    }

    #[test]
    fn a_skys_own_resolvable_panels_are_kept_over_the_default() {
        let properties: Vec<(&str, Variant)> = SKY_FACES
            .iter()
            .map(|face| {
                (
                    face.property(),
                    Variant::String(format!(
                        "rbxasset://textures/custom_{}.png",
                        face.property()
                    )),
                )
            })
            .collect();
        let (dom, referent) = sky_with(&properties);

        let panels = panels_or_default(&dom, Some(referent));

        assert_eq!(panels.len(), 6);
        for (reference, _) in &panels {
            let AssetRef::Native(path) = reference else {
                panic!("expected a native reference");
            };
            assert!(path.starts_with("textures/custom_"), "{path}");
        }
    }

    #[test]
    fn six_distinct_native_paths_one_per_face() {
        let panels = panels();
        assert_eq!(panels.len(), 6);

        let mut paths: Vec<String> = panels
            .iter()
            .map(|(reference, _)| match reference {
                AssetRef::Native(path) => path.clone(),
                other => panic!("expected a native reference, got {other:?}"),
            })
            .collect();
        paths.sort_unstable();
        paths.dedup();
        assert_eq!(paths.len(), 6);

        for path in &paths {
            assert!(path.starts_with("sky/sky512_"), "{path}");
            assert!(path.ends_with(".tex"), "{path}");
        }
    }

    #[test]
    fn each_default_panel_matches_its_face_own_quad() {
        for (reference, panel_quad) in panels() {
            let AssetRef::Native(path) = reference else {
                panic!("expected a native reference");
            };
            let face = SKY_FACES
                .into_iter()
                .find(|face| path == format!("sky/sky512_{}.tex", face.default_suffix()))
                .unwrap_or_else(|| panic!("no face matches {path}"));
            assert_eq!(panel_quad, quad(face));
        }
    }
}
