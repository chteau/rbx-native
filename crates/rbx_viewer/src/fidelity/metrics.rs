//! How far two frames are apart: mean and 90th-percentile CIEDE2000 colour
//! difference, and SSIM over luma, with Roblox's top bar left out.

use serde_json::json;

/// An RGBA8 picture.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Self {
        assert_eq!(rgba.len(), (width * height * 4) as usize, "RGBA8 size");
        Image {
            width,
            height,
            rgba,
        }
    }

    fn pixel(&self, x: u32, y: u32) -> [u8; 3] {
        let i = ((y * self.width + x) * 4) as usize;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2]]
    }

    /// A box average down to `width × height` (each a whole divisor of the
    /// picture's own size).
    pub fn downsample(&self, width: u32, height: u32) -> Image {
        let (fx, fy) = (self.width / width, self.height / height);
        assert!(
            fx >= 1 && fy >= 1 && fx * width == self.width && fy * height == self.height,
            "{}×{} does not divide into {width}×{height}",
            self.width,
            self.height
        );
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let mut sum = [0u32; 3];
                for dy in 0..fy {
                    for dx in 0..fx {
                        let p = self.pixel(x * fx + dx, y * fy + dy);
                        for c in 0..3 {
                            sum[c] += u32::from(p[c]);
                        }
                    }
                }
                let n = fx * fy;
                rgba.extend(sum.map(|s| ((s + n / 2) / n) as u8));
                rgba.push(255);
            }
        }
        Image::new(width, height, rgba)
    }

    /// The first row below Roblox's top bar.
    fn first_row(&self) -> u32 {
        (self.height as f32 * super::TOP_BAR).ceil() as u32
    }
}

/// What [`compare`] measured.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    /// Mean CIEDE2000 over every compared pixel.
    pub delta_e: f32,
    /// The 90th percentile of the same.
    pub delta_e_p90: f32,
    /// Mean SSIM of luma over 8 × 8 windows (1 is identical).
    pub ssim: f32,
}

impl Metrics {
    pub fn to_json(self) -> serde_json::Value {
        json!({
            "delta_e": round(self.delta_e),
            "delta_e_p90": round(self.delta_e_p90),
            "ssim": round(self.ssim),
        })
    }
}

fn round(value: f32) -> f64 {
    (f64::from(value) * 1000.0).round() / 1000.0
}

/// Compares two pictures of the same size, below the top bar.
pub fn compare(real: &Image, ours: &Image) -> Metrics {
    assert_eq!((real.width, real.height), (ours.width, ours.height));
    let mut deltas = delta_map(real, ours);
    let delta_e = deltas.iter().sum::<f32>() / deltas.len().max(1) as f32;
    deltas.sort_by(f32::total_cmp);
    let delta_e_p90 = deltas
        .get(deltas.len() * 9 / 10)
        .copied()
        .unwrap_or_default();
    Metrics {
        delta_e,
        delta_e_p90,
        ssim: ssim(real, ours),
    }
}

/// CIEDE2000 per pixel below the top bar, row by row.
pub fn delta_map(real: &Image, ours: &Image) -> Vec<f32> {
    let mut out = Vec::new();
    for y in real.first_row()..real.height {
        for x in 0..real.width {
            out.push(ciede2000(lab(real.pixel(x, y)), lab(ours.pixel(x, y))));
        }
    }
    out
}

/// The mean sRGB colour of `rect` (fractions of the frame).
pub fn mean_color(image: &Image, [x0, y0, x1, y1]: [f32; 4]) -> [u8; 3] {
    let w = image.width as f32;
    let h = image.height as f32;
    let (xa, xb) = ((x0 * w) as u32, ((x1 * w) as u32).max((x0 * w) as u32 + 1));
    let (ya, yb) = ((y0 * h) as u32, ((y1 * h) as u32).max((y0 * h) as u32 + 1));
    let mut sum = [0u64; 3];
    let mut n = 0u64;
    for y in ya..yb.min(image.height) {
        for x in xa..xb.min(image.width) {
            let p = image.pixel(x, y);
            for c in 0..3 {
                sum[c] += u64::from(p[c]);
            }
            n += 1;
        }
    }
    sum.map(|s| (s / n.max(1)) as u8)
}

/// CIE Lab (D65) of an sRGB colour.
pub fn lab(rgb: [u8; 3]) -> [f32; 3] {
    let [r, g, b] = rgb.map(|c| {
        let c = f32::from(c) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    });
    let x = (0.412_456_4 * r + 0.357_576_1 * g + 0.180_437_5 * b) / 0.950_47;
    let y = 0.212_672_9 * r + 0.715_152_2 * g + 0.072_175 * b;
    let z = (0.019_333_9 * r + 0.119_192 * g + 0.950_304_1 * b) / 1.088_83;
    let f = |t: f32| {
        if t > 216.0 / 24389.0 {
            t.cbrt()
        } else {
            (24389.0 / 27.0 * t + 16.0) / 116.0
        }
    };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// The CIEDE2000 difference (Sharma, Wu and Dalal's formulation).
pub fn ciede2000([l1, a1, b1]: [f32; 3], [l2, a2, b2]: [f32; 3]) -> f32 {
    use std::f32::consts::PI;
    let c1 = a1.hypot(b1);
    let c2 = a2.hypot(b2);
    let c_bar7 = ((c1 + c2) / 2.0).powi(7);
    let g = 0.5 * (1.0 - (c_bar7 / (c_bar7 + 25f32.powi(7))).sqrt());
    let (a1, a2) = (a1 * (1.0 + g), a2 * (1.0 + g));
    let (c1, c2) = (a1.hypot(b1), a2.hypot(b2));
    let hue = |b: f32, a: f32| {
        if a == 0.0 && b == 0.0 {
            0.0
        } else {
            b.atan2(a).rem_euclid(2.0 * PI)
        }
    };
    let (h1, h2) = (hue(b1, a1), hue(b2, a2));
    let dl = l2 - l1;
    let dc = c2 - c1;
    let dh = if c1 * c2 == 0.0 {
        0.0
    } else {
        let d = h2 - h1;
        if d > PI {
            d - 2.0 * PI
        } else if d < -PI {
            d + 2.0 * PI
        } else {
            d
        }
    };
    let dh_big = 2.0 * (c1 * c2).sqrt() * (dh / 2.0).sin();
    let l_bar = (l1 + l2) / 2.0;
    let c_bar = (c1 + c2) / 2.0;
    let h_bar = if c1 * c2 == 0.0 {
        h1 + h2
    } else if (h1 - h2).abs() <= PI {
        (h1 + h2) / 2.0
    } else if h1 + h2 < 2.0 * PI {
        (h1 + h2 + 2.0 * PI) / 2.0
    } else {
        (h1 + h2 - 2.0 * PI) / 2.0
    };
    let t = 1.0 - 0.17 * (h_bar - 30f32.to_radians()).cos()
        + 0.24 * (2.0 * h_bar).cos()
        + 0.32 * (3.0 * h_bar + 6f32.to_radians()).cos()
        - 0.20 * (4.0 * h_bar - 63f32.to_radians()).cos();
    let d_theta = 30f32.to_radians() * (-((h_bar.to_degrees() - 275.0) / 25.0).powi(2)).exp();
    let c_bar7 = c_bar.powi(7);
    let r_c = 2.0 * (c_bar7 / (c_bar7 + 25f32.powi(7))).sqrt();
    let s_l = 1.0 + 0.015 * (l_bar - 50.0).powi(2) / (20.0 + (l_bar - 50.0).powi(2)).sqrt();
    let s_c = 1.0 + 0.045 * c_bar;
    let s_h = 1.0 + 0.015 * c_bar * t;
    let r_t = -(2.0 * d_theta).sin() * r_c;
    let (l, c, h) = (dl / s_l, dc / s_c, dh_big / s_h);
    (l * l + c * c + h * h + r_t * c * h).max(0.0).sqrt()
}

fn luma(image: &Image) -> Vec<f32> {
    (0..image.width * image.height)
        .map(|i| {
            let p = &image.rgba[(i * 4) as usize..];
            0.299 * f32::from(p[0]) + 0.587 * f32::from(p[1]) + 0.114 * f32::from(p[2])
        })
        .collect()
}

/// Mean SSIM over 8 × 8 windows every 4 pixels, below the top bar.
pub fn ssim(real: &Image, ours: &Image) -> f32 {
    const WINDOW: u32 = 8;
    const STEP: u32 = 4;
    const C1: f32 = (0.01 * 255.0) * (0.01 * 255.0);
    const C2: f32 = (0.03 * 255.0) * (0.03 * 255.0);
    let (a, b) = (luma(real), luma(ours));
    let width = real.width;
    let mut total = 0.0;
    let mut windows = 0u32;
    let mut y = real.first_row();
    while y + WINDOW <= real.height {
        let mut x = 0;
        while x + WINDOW <= width {
            let mut s = [0.0f32; 5];
            for dy in 0..WINDOW {
                for dx in 0..WINDOW {
                    let i = ((y + dy) * width + x + dx) as usize;
                    let (p, q) = (a[i], b[i]);
                    s[0] += p;
                    s[1] += q;
                    s[2] += p * p;
                    s[3] += q * q;
                    s[4] += p * q;
                }
            }
            let n = (WINDOW * WINDOW) as f32;
            let (ma, mb) = (s[0] / n, s[1] / n);
            let va = s[2] / n - ma * ma;
            let vb = s[3] / n - mb * mb;
            let cov = s[4] / n - ma * mb;
            total += ((2.0 * ma * mb + C1) * (2.0 * cov + C2))
                / ((ma * ma + mb * mb + C1) * (va + vb + C2));
            windows += 1;
            x += STEP;
        }
        y += STEP;
    }
    total / windows.max(1) as f32
}

/// ΔE as a picture: black at 0, through red and yellow to white at 20 and
/// beyond; the top bar left grey.
pub fn heatmap(real: &Image, ours: &Image) -> Image {
    let deltas = delta_map(real, ours);
    let first = real.first_row();
    let mut rgba = vec![64u8; (real.width * first * 4) as usize];
    for chunk in rgba.chunks_mut(4) {
        chunk[3] = 255;
    }
    for d in deltas {
        let t = (d / 20.0).clamp(0.0, 1.0) * 3.0;
        let channel = |from: f32| ((t - from).clamp(0.0, 1.0) * 255.0) as u8;
        rgba.extend([channel(0.0), channel(1.0), channel(2.0), 255]);
    }
    Image::new(real.width, real.height, rgba)
}

/// Pictures of one size laid side by side.
pub fn side_by_side(images: &[&Image]) -> Image {
    let (width, height) = (images[0].width, images[0].height);
    let total = width * images.len() as u32;
    let mut rgba = Vec::with_capacity((total * height * 4) as usize);
    for y in 0..height {
        for image in images {
            let start = (y * width * 4) as usize;
            rgba.extend_from_slice(&image.rgba[start..start + (width * 4) as usize]);
        }
    }
    Image::new(total, height, rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ciede2000_matches_sharmas_published_pairs() {
        // Pairs 1, 7 and 17 of Sharma, Wu and Dalal's test data.
        let cases = [
            ([50.0, 2.6772, -79.7751], [50.0, 0.0, -82.7485], 2.0425),
            ([50.0, 0.0, 0.0], [50.0, -1.0, 2.0], 2.3669),
            ([50.0, 2.5, 0.0], [73.0, 25.0, -18.0], 27.1492),
        ];
        for (a, b, want) in cases {
            let got = ciede2000(a, b);
            assert!((got - want).abs() < 1e-3, "{a:?} {b:?}: {got} vs {want}");
        }
    }

    #[test]
    fn white_is_lab_100_and_identical_pictures_score_perfectly() {
        let [l, a, b] = lab([255, 255, 255]);
        assert!((l - 100.0).abs() < 0.01 && a.abs() < 0.01 && b.abs() < 0.01);
        let image = Image::new(
            32,
            32,
            (0..32 * 32)
                .flat_map(|i| [(i % 251) as u8, 40, 200, 255])
                .collect(),
        );
        let metrics = compare(&image, &image);
        assert_eq!(metrics.delta_e, 0.0);
        assert!((metrics.ssim - 1.0).abs() < 1e-5);
    }

    #[test]
    fn a_box_average_keeps_the_mean() {
        let image = Image::new(
            2,
            2,
            vec![
                0, 0, 0, 255, 100, 100, 100, 255, 200, 0, 0, 255, 100, 0, 0, 255,
            ],
        );
        assert_eq!(image.downsample(1, 1).rgba, vec![100, 25, 25, 255]);
    }
}
