use std::path::Path;

use cosmic::iced::Color;

/// Produce a restrained representative color suitable for tinting controls.
/// Artwork is reduced first, so this work only runs once per cover change.
pub fn dominant_album_color(path: &Path) -> Option<Color> {
    let image = image::open(path).ok()?.to_rgba8();
    let thumbnail = image::imageops::thumbnail(&image, 32, 32);
    let mut red = 0.0;
    let mut green = 0.0;
    let mut blue = 0.0;
    let mut weight_total = 0.0;

    for pixel in thumbnail.pixels() {
        let [r, g, b, alpha] = pixel.0;
        if alpha < 24 {
            continue;
        }
        let saturation = color_spread(r, g, b);
        let lightness = (f32::from(r.max(g).max(b)) + f32::from(r.min(g).min(b))) / 510.0;
        if saturation < 0.18 || !(0.12..=0.82).contains(&lightness) {
            continue;
        }
        let weight = saturation * (1.0 - (lightness - 0.5).abs() * 2.0).max(0.0);
        red += f32::from(r) * weight;
        green += f32::from(g) * weight;
        blue += f32::from(b) * weight;
        weight_total += weight;
    }

    (weight_total > f32::EPSILON).then(|| {
        Color::from_rgb8(
            (red / weight_total).round() as u8,
            (green / weight_total).round() as u8,
            (blue / weight_total).round() as u8,
        )
    })
}

fn color_spread(r: u8, g: u8, b: u8) -> f32 {
    let high = r.max(g).max(b);
    let low = r.min(g).min(b);
    if high == 0 {
        0.0
    } else {
        f32::from(high - low) / f32::from(high)
    }
}
