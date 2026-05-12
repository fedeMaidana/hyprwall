use fontdue::Font;

use crate::{
    geometry::Rect,
    style::{self, Color},
};

/// Pinta el texto centrado dentro de `rect`, recortándolo con elipsis si no
/// entra. Trabaja sobre un buffer **RGBA** (orden de bytes R, G, B, A — el de
/// `tiny_skia::Pixmap::data_mut()`). El swap a BGRA para Wayland lo hace el
/// rasterizer al final, no acá.
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

                blend_pixel_rgba(
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

fn scale_alpha(alpha: u8, coverage: u8) -> u8 {
    ((alpha as u16 * coverage as u16) / 255) as u8
}

/// Source-over blend de un pixel RGBA sobre el canvas RGBA. Alpha
/// non-premultiplied; el resultado queda premultiplicado-equivalente porque
/// la salida de tiny-skia que rodea estos pixels también lo está para alpha=255
/// (típico). Suficiente para texto sobre fondos opacos.
fn blend_pixel_rgba(canvas: &mut [u8], surface_w: u32, x: i32, y: i32, color: Color) {
    if color.a == 0 || x < 0 || y < 0 {
        return;
    }

    let idx = ((y as u32 * surface_w + x as u32) * 4) as usize;
    if idx + 3 >= canvas.len() {
        return;
    }

    let dst_r = canvas[idx] as u16;
    let dst_g = canvas[idx + 1] as u16;
    let dst_b = canvas[idx + 2] as u16;
    let dst_a = canvas[idx + 3] as u16;

    let a = color.a as u16;
    let inv_a = 255 - a;

    let out_r = (color.r as u16 * a + dst_r * inv_a) / 255;
    let out_g = (color.g as u16 * a + dst_g * inv_a) / 255;
    let out_b = (color.b as u16 * a + dst_b * inv_a) / 255;
    let out_a = a + (dst_a * inv_a) / 255;

    canvas[idx] = out_r as u8;
    canvas[idx + 1] = out_g as u8;
    canvas[idx + 2] = out_b as u8;
    canvas[idx + 3] = out_a as u8;
}
