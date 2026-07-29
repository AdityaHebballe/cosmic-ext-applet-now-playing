use cosmic::iced::{Background, Color};
use cosmic::widget::button;

/// Blend an album color into COSMIC's existing applet style while preserving
/// the configured surface, contrast and interaction states.
pub fn album_tinted_button(mut base: button::Style, album: Option<Color>) -> button::Style {
    let Some(album) = album else { return base };
    let surface = match base.background {
        Some(Background::Color(color)) => color,
        _ => Color::from_rgb8(36, 38, 42),
    };
    let mixed = blend(surface, album, 0.28);
    base.background = Some(Background::Color(Color { a: 0.72, ..mixed }));
    base.border_width = base.border_width.max(1.0);
    base.border_color = Color {
        a: 0.78,
        ..blend(surface, album, 0.52)
    };
    base
}

fn blend(a: Color, b: Color, ratio: f32) -> Color {
    let inverse = 1.0 - ratio;
    Color {
        r: a.r * inverse + b.r * ratio,
        g: a.g * inverse + b.g * ratio,
        b: a.b * inverse + b.b * ratio,
        a: a.a * inverse + b.a * ratio,
    }
}
