//! `RBX_STUDIO_UPDATE_PLAN_MOCK`'s experiences and their icons, drawn here
//! so captures show something icon-like without any Roblox art.

use image::RgbaImage;
use rbx_cloud::{Experience, Owner, Visibility};

const GAMES: [(u64, &str, &str, &str); 3] = [
    (4_810_001, "Pebble Kart", "#ff8a3d", "#c2309b"),
    (4_810_002, "Lantern Isles", "#2fc7a5", "#1b4fd1"),
    (4_810_003, "Sky Forge Tycoon", "#ffd24a", "#e2463b"),
];

pub(crate) fn experiences() -> Vec<Experience> {
    GAMES
        .iter()
        .enumerate()
        .map(|(i, (id, name, _, _))| Experience {
            universe_id: *id,
            root_place_id: id + 100,
            name: name.to_string(),
            visibility: Visibility::Public,
            owner: if i == 2 {
                Owner::Group(77)
            } else {
                Owner::User(1)
            },
        })
        .collect()
}

pub(crate) fn icon(universe_id: u64) -> Option<RgbaImage> {
    let (_, name, from, to) = GAMES.iter().find(|g| g.0 == universe_id)?;
    let initial = name.chars().next().unwrap_or('?');
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512">
<defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="{from}"/><stop offset="1" stop-color="{to}"/></linearGradient></defs>
<rect width="512" height="512" fill="url(#g)"/>
<circle cx="420" cy="90" r="150" fill="#ffffff" fill-opacity="0.12"/>
<circle cx="70" cy="470" r="190" fill="#000000" fill-opacity="0.12"/>
<path d="M0 360 Q128 300 256 350 T512 330 V512 H0 Z" fill="#000000" fill-opacity="0.18"/>
<text x="256" y="300" text-anchor="middle" font-family="Manrope" font-weight="700" font-size="220" fill="#ffffff">{initial}</text></svg>"##
    );
    super::overlay::rasterize(&svg)
}
