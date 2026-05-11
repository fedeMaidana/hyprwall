use fontdue::Font;

use crate::{
    geometry::Rect,
    render::primitives::{blend_pixel, scale_alpha},
    style::{self, Color},
};

pub fn draw_text_centered_in_rect(
    canvas: &mut [u8],
    surface_w: u32,
    surface_h: u32,
    font: &Font,
    text: &str,
    font_size: f32,
    rect: Rect,
    color: Color,
) {
    let text = fit_text_to_width(font, text, font_size, rect.w as f32);
    let text_w = measure_text_width(font, &text, font_size);

    let Some(line_metrics) = font.horizontal_line_metrics(font_size) else {
        return;
    };

    let line_h = line_metrics.ascent - line_metrics.descent;
    let baseline = rect.y as f32 + (rect.h as f32 - line_h) / 2.0 + line_metrics.ascent;

    let x = rect.x as f32 + (rect.w as f32 - text_w) / 2.0;

    draw_text_baseline(
        canvas, surface_w, surface_h, font, &text, font_size, x, baseline, color,
    );
}

fn draw_text_baseline(
    canvas: &mut [u8],
    surface_w: u32,
    surface_h: u32,
    font: &Font,
    text: &str,
    font_size: f32,
    x: f32,
    baseline: f32,
    color: Color,
) {
    let mut pen_x = x;

    for ch in text.chars() {
        let (metrics, bitmap) = font.rasterize(ch, font_size);

        let glyph_x = pen_x.round() as i32 + metrics.xmin;
        let glyph_y = (baseline - metrics.ymin as f32 - metrics.height as f32).round() as i32;

        for gy in 0..metrics.height {
            for gx in 0..metrics.width {
                let alpha = bitmap[gy * metrics.width + gx];

                if alpha == 0 {
                    continue;
                }

                let px = glyph_x + gx as i32;
                let py = glyph_y + gy as i32;

                if px < 0 || py < 0 || px >= surface_w as i32 || py >= surface_h as i32 {
                    continue;
                }

                blend_pixel(
                    canvas,
                    surface_w,
                    px,
                    py,
                    Color {
                        a: scale_alpha(color.a, alpha),
                        ..color
                    },
                );
            }
        }

        pen_x += metrics.advance_width;

        // Compactamos un poco solo entre letras, no después de espacios.
        if !ch.is_whitespace() {
            pen_x += style::label::LETTER_SPACING;
        }
    }
}

fn measure_text_width(font: &Font, text: &str, font_size: f32) -> f32 {
    let mut width = 0.0;
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        let metrics = font.metrics(ch, font_size);
        width += metrics.advance_width;

        if chars.peek().is_some() && !ch.is_whitespace() {
            width += style::label::LETTER_SPACING;
        }
    }

    width.max(0.0)
}

fn fit_text_to_width(font: &Font, text: &str, font_size: f32, max_width: f32) -> String {
    if measure_text_width(font, text, font_size) <= max_width {
        return text.to_owned();
    }

    let ellipsis = "…";
    let ellipsis_w = measure_text_width(font, ellipsis, font_size);

    let mut fitted = String::new();

    for ch in text.chars() {
        let next = format!("{fitted}{ch}");

        if measure_text_width(font, &next, font_size) + ellipsis_w > max_width {
            break;
        }

        fitted.push(ch);
    }

    fitted.push('…');
    fitted
}
