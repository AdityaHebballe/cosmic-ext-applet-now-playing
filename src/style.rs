use cosmic::iced::{Background, Color};
use cosmic::widget::button;

pub fn applet_button(mut base: button::Style, color: Option<Color>) -> button::Style {
    if let Some(album) = color {
        let theme_base = match base.background {
            Some(Background::Color(color)) => color,
            _ => Color::from_rgb8(36, 38, 42),
        };

        let mixed = blend_color(theme_base, album, 0.36);
        let background = with_alpha(mixed, 0.64);
        let border = with_alpha(shift_color(album, -0.05), 0.82);
        let foreground = contrast_text_color(background);

        base.background = Some(Background::Color(background));
        base.border_width = base.border_width.max(1.0);
        base.border_color = border;
        base.text_color = Some(foreground);
        base.icon_color = Some(foreground);
    }

    base
}

#[must_use]
pub fn shift_color(color: Color, amount: f32) -> Color {
    let adjust = |channel: f32| (channel + amount).clamp(0.0, 1.0);
    Color {
        r: adjust(color.r),
        g: adjust(color.g),
        b: adjust(color.b),
        a: color.a,
    }
}

fn with_alpha(mut color: Color, alpha: f32) -> Color {
    color.a = alpha.clamp(0.0, 1.0);
    color
}

fn blend_color(a: Color, b: Color, ratio: f32) -> Color {
    let ratio = ratio.clamp(0.0, 1.0);
    let inverse = 1.0 - ratio;
    Color {
        r: a.r.mul_add(inverse, b.r * ratio),
        g: a.g.mul_add(inverse, b.g * ratio),
        b: a.b.mul_add(inverse, b.b * ratio),
        a: a.a.mul_add(inverse, b.a * ratio),
    }
}

fn contrast_text_color(background: Color) -> Color {
    let luminance = 0.2126f32.mul_add(
        background.r,
        0.7152f32.mul_add(background.g, 0.0722 * background.b),
    );
    if luminance > 0.58 {
        Color::from_rgb8(17, 17, 17)
    } else {
        Color::WHITE
    }
}
