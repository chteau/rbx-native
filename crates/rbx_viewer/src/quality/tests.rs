use super::*;

/// Every level, as the renderer would see it.
fn profiles() -> Vec<QualityProfile> {
    (QualityLevel::MIN..=QualityLevel::MAX)
        .map(|level| QualityLevel::Level(level).profile())
        .collect()
}

#[test]
fn every_level_in_range_resolves_to_itself() {
    for level in QualityLevel::MIN..=QualityLevel::MAX {
        assert_eq!(QualityLevel::Level(level).resolved(), level);
    }
}

#[test]
fn a_level_outside_the_range_is_clamped_rather_than_lost() {
    assert_eq!(QualityLevel::Level(0).resolved(), QualityLevel::MIN);
    assert_eq!(QualityLevel::Level(99).resolved(), QualityLevel::MAX);
}

// Resolving Automatic is the path screenshots and fixtures take, and it must be
// the top level rather than a guess; a host with a frame clock never resolves it
// at all, it drives `FrameRateManager` and sets the level itself.
#[test]
fn automatic_and_the_default_are_both_the_top_level() {
    assert_eq!(QualityLevel::Automatic.resolved(), QualityLevel::MAX);
    assert_eq!(QualityLevel::default().resolved(), QualityLevel::MAX);
    assert_eq!(
        QualityLevel::Automatic.profile(),
        QualityLevel::Level(QualityLevel::MAX).profile()
    );
}

#[test]
fn the_words_and_the_numbers_both_parse() {
    for text in ["auto", "AUTO", "automatic", " Automatic "] {
        assert_eq!(text.parse(), Ok(QualityLevel::Automatic), "{text}");
    }
    for text in ["7", "07", "Level07", "level7", "LEVEL07"] {
        assert_eq!(text.parse(), Ok(QualityLevel::Level(7)), "{text}");
    }
    assert_eq!("21".parse(), Ok(QualityLevel::Level(21)));
    assert_eq!("1".parse(), Ok(QualityLevel::Level(1)));
}

#[test]
fn anything_outside_the_enum_is_refused() {
    for text in ["0", "22", "300", "", "level", "high", "ultra", "-1", "7.5"] {
        assert!(text.parse::<QualityLevel>().is_err(), "{text}");
    }
}

/// The whole promise of the table: no knob may get *worse* as the level rises.
/// A row edited by hand is exactly where that would break.
#[test]
fn every_knob_only_ever_improves_with_the_level() {
    for pair in profiles().windows(2) {
        let [low, high] = [pair[0], pair[1]];
        assert!(!low.shadows || high.shadows);
        assert!(low.shadow_map_size <= high.shadow_map_size);
        assert!(low.shadow_pcf_radius_scale <= high.shadow_pcf_radius_scale);
        assert!(low.shadow_distance <= high.shadow_distance);
        assert!(!low.bloom || high.bloom);
        assert!(!low.color_correction || high.color_correction);
        assert!(low.local_lights_max <= high.local_lights_max);
        assert!(low.local_shadow_lights_max <= high.local_shadow_lights_max);
        assert!(low.texture_max_size <= high.texture_max_size);
        assert!(low.anisotropy <= high.anisotropy);
        assert!(!low.env_reflections || high.env_reflections);
        assert!(low.render_distance <= high.render_distance);
        assert!(low.msaa_samples <= high.msaa_samples);
        assert!(!low.decals || high.decals);
    }
}

#[test]
fn shadows_appear_at_five_and_never_switch_off_again() {
    for level in QualityLevel::MIN..5 {
        assert!(!QualityLevel::Level(level).profile().shadows, "{level}");
    }
    for level in 5..=QualityLevel::MAX {
        assert!(QualityLevel::Level(level).profile().shadows, "{level}");
    }
}

// Shadows arrive hard-edged and only start honouring the place's own
// `ShadowSoftness` at 7, which is also where the map doubles.
#[test]
fn the_map_grows_and_softens_in_step_with_the_bands() {
    let profile = |level| QualityLevel::Level(level).profile();

    assert_eq!(profile(6).shadow_map_size, 1024);
    assert_eq!(profile(6).shadow_pcf_radius_scale, table::HARD_SHADOW_EDGE);
    assert_eq!(profile(7).shadow_map_size, 2048);
    assert_eq!(
        profile(7).shadow_pcf_radius_scale,
        table::PLACE_SHADOW_SOFTNESS
    );
    assert_eq!(profile(9).shadow_map_size, 2048);
    assert_eq!(profile(10).shadow_map_size, 4096);
    // The top band doubles the map again rather than just the distance (see
    // `table::FAR_SHADOWS_STUDS`'s doc comment): otherwise max quality would
    // hand back the same texel density as level 9, not an improvement on it.
    assert_eq!(profile(QualityLevel::MAX).shadow_map_size, 8192);
}

#[test]
fn the_shadow_distance_steps_at_nine_and_at_sixteen() {
    let distance = |level| QualityLevel::Level(level).profile().shadow_distance;

    assert_eq!(distance(8), table::NEAR_SHADOWS_STUDS);
    assert_eq!(distance(9), table::MID_SHADOWS_STUDS);
    assert_eq!(distance(15), table::MID_SHADOWS_STUDS);
    assert_eq!(distance(16), table::FAR_SHADOWS_STUDS);
}

// Neon's glow is the tell for the bloom, and the community's reading puts it at
// level 5 — the same level shadows arrive at.
#[test]
fn the_bloom_arrives_at_five_and_the_grade_at_three() {
    assert!(!QualityLevel::Level(4).profile().bloom);
    assert!(QualityLevel::Level(5).profile().bloom);
    assert!(!QualityLevel::Level(2).profile().color_correction);
    assert!(QualityLevel::Level(3).profile().color_correction);
}

#[test]
fn the_local_light_cap_lifts_band_by_band_and_ends_unlimited() {
    let cap = |level| QualityLevel::Level(level).profile().local_lights_max;

    assert_eq!(cap(1), 0);
    assert_eq!(cap(2), 0);
    assert_eq!(cap(3), 8);
    assert_eq!(cap(5), 64);
    assert_eq!(cap(7), 256);
    assert_eq!(cap(9), 256);
    assert_eq!(cap(10), usize::MAX);
    assert_eq!(cap(QualityLevel::MAX), usize::MAX);
}

// One map per shadowed light, so the cap is also the shadow texture array's
// layer count: it has to stay a handful of small bands, not grow with the
// place's own light count the way `local_lights_max` does.
#[test]
fn the_local_shadow_cap_lifts_at_seven_ten_and_sixteen() {
    let cap = |level| QualityLevel::Level(level).profile().local_shadow_lights_max;

    assert_eq!(cap(6), 0);
    assert_eq!(cap(7), 4);
    assert_eq!(cap(9), 4);
    assert_eq!(cap(10), 8);
    assert_eq!(cap(15), 8);
    assert_eq!(cap(16), 16);
    assert_eq!(cap(QualityLevel::MAX), 16);
}

// "Apparent texture resolution" is the one thing Roblox's own wording is
// explicit about, and it is what these two bands are.
#[test]
fn texture_detail_climbs_in_two_steps_and_anisotropy_in_two_more() {
    let profile = |level| QualityLevel::Level(level).profile();

    assert_eq!(profile(2).texture_max_size, 256);
    assert_eq!(profile(3).texture_max_size, 512);
    assert_eq!(profile(6).texture_max_size, 512);
    assert_eq!(profile(7).texture_max_size, 1024);
    assert_eq!(profile(QualityLevel::MAX).texture_max_size, 1024);

    assert_eq!(profile(4).anisotropy, 1);
    assert_eq!(profile(5).anisotropy, 4);
    assert_eq!(profile(9).anisotropy, 4);
    assert_eq!(profile(10).anisotropy, 16);
}

// Reflections are the community's marker for level 8 (glass and water "below 8"
// being the same boundary), and the view distance only stops being a limit at the
// top band — which is what keeps a reference capture identical.
#[test]
fn reflections_start_at_eight_and_the_view_distance_ends_unlimited() {
    let profile = |level| QualityLevel::Level(level).profile();

    assert!(!profile(7).env_reflections);
    assert!(profile(8).env_reflections);

    assert_eq!(profile(2).render_distance, table::NEAR_RENDER_STUDS);
    assert_eq!(profile(6).render_distance, table::MID_RENDER_STUDS);
    assert_eq!(profile(9).render_distance, table::FAR_RENDER_STUDS);
    assert_eq!(profile(15).render_distance, table::DISTANT_RENDER_STUDS);
    assert!(profile(16).render_distance.is_infinite());
    assert!(profile(QualityLevel::MAX).render_distance.is_infinite());
}

// The one band where the surface pipelines differ, which is why the boundary is
// worth pinning: a switch across it rebuilds them.
#[test]
fn multisampling_starts_at_the_top_band() {
    assert_eq!(QualityLevel::Level(15).profile().msaa_samples, 1);
    assert_eq!(QualityLevel::Level(16).profile().msaa_samples, 4);
}

#[test]
fn decals_survive_every_level() {
    assert!(profiles().iter().all(|profile| profile.decals));
}

// `renderer::texture` uploads to `table::MAX_TEXTURE_SIZE` and relies on no
// band ever viewing past it — a band that did would be silently cropped.
#[test]
fn no_band_asks_to_view_past_the_upload_cap() {
    assert!(profiles()
        .iter()
        .all(|profile| profile.texture_max_size <= table::MAX_TEXTURE_SIZE));
}
