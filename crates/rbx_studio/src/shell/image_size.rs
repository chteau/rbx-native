//! An image property's resolution beside its id in Properties (`512×512`),
//! and the image itself in that label's tooltip. The image comes the way
//! the viewport gets it (`rbx_viewer::decode_image_uri`): the shared asset
//! cache first, so one the place already shows costs no download. Each URI
//! is measured once per session.

use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;
use rbx_assets::AssetRef;

use crate::properties::{EditKind, PropertyRow};
use crate::tokens;

use super::Shell;

/// The longest side of the tooltip's picture, in px.
const THUMB: u32 = 160;

/// Content properties that hold an image, by name, less the `Content`
/// suffix the newer twin of each one carries (`Image`/`ImageContent`).
/// Every class names them the same (`Decal.Texture`, `Texture.Texture`,
/// `ParticleEmitter.Texture`, ...), so the name is enough; the skybox's six
/// all start `Skybox`.
const IMAGE_PROPERTIES: [&str; 34] = [
    "Texture",
    "TextureId",
    "TextureID",
    "Image",
    "HoverImage",
    "PressedImage",
    "TopImage",
    "MidImage",
    "BottomImage",
    "ColorMap",
    "NormalMap",
    "MetalnessMap",
    "RoughnessMap",
    "EmissiveMask",
    "ShirtTemplate",
    "PantsTemplate",
    "Graphic",
    "SunTexture",
    "SunTextureId",
    "MoonTexture",
    "MoonTextureId",
    "CursorIcon",
    "ActivatedCursorIcon",
    "MouseIcon",
    "Icon",
    "CameraButtonIcon",
    "FallbackImage",
    "LocalizedTexture",
    "LocalizedImage",
    "BaseTexture",
    "OverlayTexture",
    "DisplayImage",
    "RewardImage",
    "ResolvedImage",
];

fn is_image_property(name: &str) -> bool {
    let base = name.strip_suffix("Content").unwrap_or(name);
    base.starts_with("Skybox") || IMAGE_PROPERTIES.contains(&base)
}

enum Measured {
    Loading,
    Ready {
        width: u32,
        height: u32,
        thumb: Arc<RenderImage>,
    },
    Failed(String),
}

#[derive(Default)]
pub(super) struct ImageSizes {
    by_uri: HashMap<String, Measured>,
}

/// The URI `row` holds when it is an image property naming an asset.
fn image_uri(row: &PropertyRow) -> Option<&str> {
    if row.mixed || !is_image_property(&row.name) {
        return None;
    }
    let text = match row.edit.as_ref()? {
        EditKind::Text(text)
        | EditKind::Content {
            object: false,
            text,
        } => text.trim(),
        _ => return None,
    };
    match AssetRef::parse(text) {
        Ok(AssetRef::Id(_) | AssetRef::Native(_)) => Some(text),
        _ => None,
    }
}

/// `width`×`height`, the way Studio writes a size.
pub(super) fn size_label(width: u32, height: u32) -> String {
    format!("{width}\u{d7}{height}")
}

impl Shell {
    /// The resolution label for `row`, or `None` when it holds no image
    /// asset. The first sight of a URI starts measuring it.
    pub(super) fn image_size_label(
        &mut self,
        row: &PropertyRow,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let uri = image_uri(row)?.to_owned();
        if !self.image_sizes.by_uri.contains_key(&uri) {
            self.image_sizes
                .by_uri
                .insert(uri.clone(), Measured::Loading);
            self.measure_image(uri.clone(), cx);
        }
        let id = SharedString::from(format!("image-size-{}", row.name));
        let label = match self.image_sizes.by_uri.get(&uri)? {
            Measured::Loading => return None,
            Measured::Ready {
                width,
                height,
                thumb,
            } => {
                let (width, height, thumb) = (*width, *height, thumb.clone());
                div()
                    .id(id)
                    .flex_none()
                    .text_size(tokens::text_xs())
                    .text_color(tokens::text2())
                    .child(size_label(width, height))
                    .tooltip(move |window, cx| {
                        let thumb = thumb.clone();
                        let uri = uri.clone();
                        Tooltip::element(move |_, _| preview(&thumb, width, height, &uri))
                            .build(window, cx)
                    })
            }
            Measured::Failed(why) => {
                let why = SharedString::from(why.clone());
                div()
                    .id(id)
                    .flex_none()
                    .text_size(tokens::text_xs())
                    .text_color(tokens::text3())
                    .child("?\u{d7}?")
                    .tooltip(move |window, cx| super::tooltip::text(why.clone(), window, cx))
            }
        };
        Some(label.into_any_element())
    }

    fn measure_image(&mut self, uri: String, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let wanted = uri.clone();
            let measured = cx.background_spawn(async move { measure(&wanted) }).await;
            let _ = this.update(cx, |shell, cx| {
                shell.image_sizes.by_uri.insert(uri, measured);
                cx.notify();
            });
        })
        .detach();
    }
}

fn measure(uri: &str) -> Measured {
    let decoded = match rbx_viewer::decode_image_uri(uri) {
        Ok(decoded) => decoded,
        Err(why) => return Measured::Failed(format!("Couldn\u{2019}t load the image: {why}")),
    };
    let (width, height) = (decoded.width, decoded.height);
    let Some(full) = image::RgbaImage::from_raw(width, height, decoded.pixels) else {
        return Measured::Failed("The image decoded short of pixels".into());
    };
    let scale = (THUMB as f32 / width.max(height).max(1) as f32).min(1.);
    let mut thumb = image::imageops::thumbnail(
        &full,
        ((width as f32 * scale).round() as u32).max(1),
        ((height as f32 * scale).round() as u32).max(1),
    );
    over_checkerboard(&mut thumb);
    let (w, h) = thumb.dimensions();
    match crate::render_image::to_render_image(thumb.into_raw(), w, h) {
        Some(thumb) => Measured::Ready {
            width,
            height,
            thumb,
        },
        None => Measured::Failed("The image decoded short of pixels".into()),
    }
}

/// Lays `image` over the grey checkerboard image editors draw under
/// transparency, so a dark decal reads on the dark tooltip as well as a light
/// one does.
fn over_checkerboard(image: &mut image::RgbaImage) {
    const SQUARE: u32 = 8;
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        let under = if (x / SQUARE + y / SQUARE).is_multiple_of(2) {
            204.
        } else {
            153.
        };
        let alpha = pixel[3] as f32 / 255.;
        for channel in 0..3 {
            pixel[channel] = (pixel[channel] as f32 * alpha + under * (1. - alpha)).round() as u8;
        }
        pixel[3] = 255;
    }
}

/// The tooltip: the picture on a tile-coloured well, its size and
/// its URI.
fn preview(thumb: &Arc<RenderImage>, width: u32, height: u32, uri: &str) -> impl IntoElement {
    let size = thumb.size(0);
    v_flex()
        .gap(px(6.))
        .child(
            h_flex()
                .justify_center()
                .items_center()
                .size(px(THUMB as f32))
                .rounded(tokens::radius())
                .bg(tokens::tile())
                .child(
                    img(thumb.clone())
                        .w(px(size.width.0 as f32))
                        .h(px(size.height.0 as f32)),
                ),
        )
        .child(
            h_flex()
                .gap(px(8.))
                .justify_between()
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(size_label(width, height)),
                )
                .child(div().text_color(tokens::text2()).child(uri.to_owned())),
        )
}

#[cfg(test)]
mod tests {
    use super::{is_image_property, size_label};

    #[test]
    fn image_properties_are_known_by_name_with_or_without_content() {
        for name in [
            "Texture",
            "ColorMapContent",
            "Image",
            "ImageContent",
            "SkyboxBk",
            "SkyboxUpContent",
            "TextureID",
        ] {
            assert!(is_image_property(name), "{name}");
        }
        for name in [
            "SoundId",
            "MeshId",
            "MeshContent",
            "AnimationId",
            "Content",
            "Name",
        ] {
            assert!(!is_image_property(name), "{name}");
        }
    }

    #[test]
    fn transparency_shows_the_checkerboard() {
        let mut image = image::RgbaImage::from_pixel(16, 1, image::Rgba([0, 0, 0, 0]));
        image.put_pixel(0, 0, image::Rgba([10, 20, 30, 255]));
        super::over_checkerboard(&mut image);
        assert_eq!(image.get_pixel(0, 0).0, [10, 20, 30, 255]);
        assert_eq!(image.get_pixel(1, 0).0, [204, 204, 204, 255]);
        assert_eq!(image.get_pixel(8, 0).0, [153, 153, 153, 255]);
    }

    #[test]
    fn a_size_reads_like_studio_writes_it() {
        assert_eq!(size_label(512, 256), "512\u{d7}256");
    }
}
