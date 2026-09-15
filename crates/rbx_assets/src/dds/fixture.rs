//! Verifies the decoder against a real Studio panel, not a hand-built one:
//! `sky\sky512_ft.tex`, extracted from `content-textures3.zip`.
//!
//! Ignored by default since it needs that file on disk; run once with
//! `RBX_SKY_FIXTURE=<path-to-extracted-tex> cargo test -p rbx_assets -- --ignored`.

use super::decode;

#[test]
#[ignore = "needs RBX_SKY_FIXTURE pointing at an extracted sky512_*.tex"]
fn decodes_the_real_default_sky_panel() {
    let path = std::env::var("RBX_SKY_FIXTURE")
        .expect("set RBX_SKY_FIXTURE to a sky512_*.tex extracted from content-textures3.zip");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"));

    let image = decode(&bytes).unwrap();

    // Despite the "512" in its file name, Studio's own default sky panel is a
    // 1024x1024 top mip — that number names which mip Studio's UI shows, not
    // this file's actual top-level resolution.
    assert_eq!(image.dimensions(), (1024, 1024));

    let row_average_rgb = |y: u32| {
        let (r, g, b) = (0..image.width())
            .map(|x| image.get_pixel(x, y).0)
            .fold((0u64, 0u64, 0u64), |(r, g, b), p| {
                (r + p[0] as u64, g + p[1] as u64, b + p[2] as u64)
            });
        let n = image.width() as u64;
        (r / n, g / n, b / n)
    };
    let (r, g, b) = row_average_rgb(0);
    println!("sky512_ft.tex top row average RGB = ({r}, {g}, {b})");
    // A skybox's zenith is sky, not ground: blue reads strongest at the top.
    assert!(b > r, "top row should read bluish, got rgb=({r},{g},{b})");

    assert!(
        image.pixels().all(|p| p.0[3] == 255),
        "a skybox panel has no transparency"
    );
}
