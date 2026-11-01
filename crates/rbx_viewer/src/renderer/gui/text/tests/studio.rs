//! Widths against Studio's own: `TextService:GetTextSize` dumped per
//! `Enum.Font` (one JSON per font, `{"font", "strings": [[size, text,
//! width]], "glyphs": [...], "pairs": [...]}`) into the directory
//! `RBX_TEXT_WIDTHS_FIXTURE` names, with the face files read from the
//! Studio install's `content/fonts` that `RBX_STUDIO_FONTS` names. Roblox
//! content, so it never lives in the repository and the test is ignored.
//! With `RBX_TEXT_WIDTHS_OUT` set, our widths for every entry are written
//! there in the same shape.

use std::path::Path;

use super::*;

/// `Enum.Font` → family file and weight.
const FONTS: [(&str, &str, u16); 7] = [
    ("BuilderSans", "BuilderSans", 400),
    ("BuilderSansBold", "BuilderSans", 700),
    ("Montserrat", "Montserrat", 400),
    ("MontserratBold", "Montserrat", 700),
    ("Arimo", "Arimo", 400),
    ("SourceSans", "SourceSansPro", 400),
    ("Roboto", "Roboto", 400),
];

fn typesetter(fonts: &Path) -> Typesetter {
    let mut library = Library::default();
    let mut wanted = Vec::new();
    for (_, family, weight) in FONTS {
        let face = Face::named(family, weight, false);
        let json = std::fs::read(fonts.join(format!("families/{family}.json"))).unwrap();
        let parsed = Family::parse(&json).unwrap();
        for entry in &parsed.faces {
            if let AssetRef::Native(path) = &entry.asset {
                let file = fonts.join(path.trim_start_matches("fonts/"));
                if let Ok(bytes) = std::fs::read(file) {
                    library.faces.insert(entry.asset.clone(), Arc::new(bytes));
                }
            }
        }
        library.families.insert(face.family.clone(), parsed);
        wanted.push(face);
    }
    let mut typesetter = Typesetter::new();
    assert!(typesetter.adopt(&library, &wanted));
    typesetter
}

fn width(fonts: &mut Typesetter, face: &Face, size: f32, text: &str) -> f32 {
    let mut gui = plain(face.clone(), size);
    gui.spans = vec![GuiTextSpan::plain(text)];
    fonts.measure(&gui, size, None)[0]
}

#[test]
#[ignore = "needs RBX_TEXT_WIDTHS_FIXTURE and RBX_STUDIO_FONTS"]
fn widths_match_studio_within_a_pixel() {
    let fixture = std::env::var("RBX_TEXT_WIDTHS_FIXTURE").expect("RBX_TEXT_WIDTHS_FIXTURE");
    let fonts_dir = std::env::var("RBX_STUDIO_FONTS").expect("RBX_STUDIO_FONTS");
    let out = std::env::var("RBX_TEXT_WIDTHS_OUT").ok();
    let mut fonts = typesetter(Path::new(&fonts_dir));
    let mut misses = Vec::new();
    for (name, family, weight) in FONTS {
        let face = Face::named(family, weight, false);
        let json = std::fs::read(Path::new(&fixture).join(format!("{name}.json"))).unwrap();
        let mut studio: serde_json::Value = serde_json::from_slice(&json).unwrap();
        let (mut total, mut worst, mut within) = (0.0f32, 0.0f32, 0);
        let mut count = 0;
        for list in ["strings", "glyphs", "pairs"] {
            let Some(entries) = studio[list].as_array_mut() else {
                continue;
            };
            for entry in entries {
                let size = entry[0].as_f64().unwrap() as f32;
                let text = entry[1].as_str().unwrap().to_string();
                let theirs = entry[2].as_f64().unwrap() as f32;
                let ours = width(&mut fonts, &face, size, &text);
                entry[2] = ours.into();
                if list != "strings" {
                    continue;
                }
                let miss = (ours - theirs).abs();
                count += 1;
                total += miss;
                worst = worst.max(miss);
                if miss <= 1.0 {
                    within += 1;
                } else {
                    misses.push(format!(
                        "{name} {size} {text:?}: ours {ours} Studio {theirs}"
                    ));
                }
            }
        }
        println!(
            "{name:16} {count} strings, mean miss {:.2} px, worst {worst} px, {within} within 1 px",
            total / count.max(1) as f32
        );
        if let Some(out) = &out {
            std::fs::write(
                Path::new(out).join(format!("{name}.json")),
                serde_json::to_vec(&studio).unwrap(),
            )
            .unwrap();
        }
    }
    // `prefixes.json`, optional: `[[font, size, text, [width of every
    // prefix]]]`, which says which glyph a miss starts at.
    if let Ok(json) = std::fs::read(Path::new(&fixture).join("prefixes.json")) {
        let rows: Vec<(String, f32, String, Vec<f32>)> = serde_json::from_slice(&json).unwrap();
        for (name, size, text, widths) in rows {
            let (_, family, weight) = FONTS.iter().find(|f| f.0 == name).unwrap();
            let face = Face::named(family, *weight, false);
            let mut last = 0.0;
            for (end, theirs) in text
                .char_indices()
                .map(|(at, c)| at + c.len_utf8())
                .zip(widths)
            {
                let miss = width(&mut fonts, &face, size, &text[..end]) - theirs;
                if miss != last {
                    println!(
                        "{name} {size} ..{:?}: {miss:+}",
                        &text[end.saturating_sub(6)..end]
                    );
                    last = miss;
                }
            }
        }
    }
    assert!(misses.is_empty(), "{}", misses.join("\n"));
}
