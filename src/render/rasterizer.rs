use fontdue::Font;
use tiny_skia::{
    FillRule, GradientStop, LinearGradient, Mask, Paint, Path, PathBuilder, Pixmap, PixmapPaint,
    PixmapRef, Point, SpreadMode, Transform,
};

use crate::{
    geometry::{Corners, Rect},
    render::{
        scene::{DrawCmd, Scene},
        text::draw_text_centered_in_rect,
    },
    style::Color,
    wallpaper::Thumbnail,
};

/// Constante de Bezier para aproximar un cuarto de círculo con una curva
/// cúbica: 4*(√2 − 1)/3.
const CIRCLE_KAPPA: f32 = 0.5522847498307933;

/// Walks a Scene in order and produces an RGBA8888 image, then copies it
/// into `canvas` doing the channel swap RGBA → BGRA that Wayland's
/// `Argb8888` format expects (little-endian => BGRA in memory).
pub fn rasterize(canvas: &mut [u8], width: u32, height: u32, scene: &Scene<'_>, font: &Font) {
    let Some(mut pixmap) = Pixmap::new(width, height) else {
        log::error!("no se pudo crear tiny-skia Pixmap {width}x{height}");
        return;
    };

    // pixmap arranca transparent (zeroed).

    for cmd in &scene.commands {
        match *cmd {
            DrawCmd::RoundRect {
                rect,
                radius,
                corners,
                color,
            } => fill_round_rect(&mut pixmap, rect, radius, corners, color),

            DrawCmd::VerticalGradient {
                rect,
                radius,
                corners,
                bottom_color,
                top_alpha,
            } => {
                fill_vertical_gradient(&mut pixmap, rect, radius, corners, bottom_color, top_alpha)
            }

            DrawCmd::Thumbnail {
                rect,
                radius,
                corners,
                thumb,
            } => draw_thumbnail(&mut pixmap, rect, radius, corners, thumb),

            DrawCmd::Text {
                rect,
                text,
                font_size,
                color,
            } => {
                // text.rs trabaja sobre el mismo buffer RGBA del pixmap.
                draw_text_centered_in_rect(
                    pixmap.data_mut(),
                    width,
                    height,
                    font,
                    text,
                    font_size,
                    rect,
                    color,
                );
            }
        }
    }

    // Pixmap está en RGBA. Wayland Argb8888 little-endian = BGRA en memoria.
    copy_rgba_to_bgra(canvas, pixmap.data());
}

fn fill_round_rect(pixmap: &mut Pixmap, rect: Rect, radius: i32, corners: Corners, color: Color) {
    let Some(path) = round_rect_path(rect, radius, corners) else {
        return;
    };

    let mut paint = Paint::default();
    paint.set_color_rgba8(color.r, color.g, color.b, color.a);
    paint.anti_alias = true;

    pixmap.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

fn fill_vertical_gradient(
    pixmap: &mut Pixmap,
    rect: Rect,
    radius: i32,
    corners: Corners,
    bottom_color: Color,
    top_alpha: u8,
) {
    if rect.h <= 0 {
        return;
    }

    let Some(path) = round_rect_path(rect, radius, corners) else {
        return;
    };

    let top =
        tiny_skia::Color::from_rgba8(bottom_color.r, bottom_color.g, bottom_color.b, top_alpha);
    let bot = tiny_skia::Color::from_rgba8(
        bottom_color.r,
        bottom_color.g,
        bottom_color.b,
        bottom_color.a,
    );

    let Some(shader) = LinearGradient::new(
        Point::from_xy(rect.x as f32, rect.y as f32),
        Point::from_xy(rect.x as f32, (rect.y + rect.h) as f32),
        vec![GradientStop::new(0.0, top), GradientStop::new(1.0, bot)],
        SpreadMode::Pad,
        Transform::identity(),
    ) else {
        return;
    };

    let mut paint = Paint::default();
    paint.shader = shader;
    paint.anti_alias = true;

    pixmap.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

fn draw_thumbnail(
    pixmap: &mut Pixmap,
    rect: Rect,
    radius: i32,
    corners: Corners,
    thumb: &Thumbnail,
) {
    if rect.w <= 0 || rect.h <= 0 || thumb.width == 0 || thumb.height == 0 {
        return;
    }

    // PixmapRef::from_bytes asume RGBA premultiplicado; nuestros thumbs son
    // RGBA non-premultiplied. Para alpha=255 (todos los wallpapers reales)
    // los bytes coinciden, así que no hay diferencia. Si algún día metemos
    // wallpapers con alpha < 255, hay que premultiplicar en el loader.
    let Some(thumb_ref) = PixmapRef::from_bytes(&thumb.rgba, thumb.width, thumb.height) else {
        return;
    };

    // Cover-fit: escalar para llenar el rect, recortando exceso.
    let scale_x = rect.w as f32 / thumb.width as f32;
    let scale_y = rect.h as f32 / thumb.height as f32;
    let scale = scale_x.max(scale_y);

    let scaled_w = thumb.width as f32 * scale;
    let scaled_h = thumb.height as f32 * scale;

    let dx = rect.x as f32 + (rect.w as f32 - scaled_w) / 2.0;
    let dy = rect.y as f32 + (rect.h as f32 - scaled_h) / 2.0;

    let transform = Transform::from_scale(scale, scale).post_translate(dx, dy);

    // Clip al rounded rect.
    let mut mask = match Mask::new(pixmap.width(), pixmap.height()) {
        Some(m) => m,
        None => return,
    };
    let Some(clip) = round_rect_path(rect, radius, corners) else {
        return;
    };
    mask.fill_path(&clip, FillRule::Winding, true, Transform::identity());

    pixmap.draw_pixmap(
        0,
        0,
        thumb_ref,
        &PixmapPaint::default(),
        transform,
        Some(&mask),
    );
}

/// Construye un path de rectángulo con esquinas redondeadas selectivas.
/// Cada esquina es independiente: las "apagadas" salen en ángulo recto.
/// Las curvas son cubics que aproximan un cuarto de círculo con kappa.
fn round_rect_path(rect: Rect, radius: i32, corners: Corners) -> Option<Path> {
    if rect.w <= 0 || rect.h <= 0 {
        return None;
    }

    let r_max = (rect.w.min(rect.h)) / 2;
    let r = radius.clamp(0, r_max) as f32;

    let x = rect.x as f32;
    let y = rect.y as f32;
    let w = rect.w as f32;
    let h = rect.h as f32;

    let tl = if corners.top_left { r } else { 0.0 };
    let tr = if corners.top_right { r } else { 0.0 };
    let br = if corners.bottom_right { r } else { 0.0 };
    let bl = if corners.bottom_left { r } else { 0.0 };

    let k = CIRCLE_KAPPA;

    let mut pb = PathBuilder::new();

    // Empezamos en el top edge, justo después del corner TL.
    pb.move_to(x + tl, y);

    // Top edge → TR corner.
    pb.line_to(x + w - tr, y);
    if tr > 0.0 {
        pb.cubic_to(
            x + w - tr + tr * k,
            y,
            x + w,
            y + tr - tr * k,
            x + w,
            y + tr,
        );
    }

    // Right edge → BR corner.
    pb.line_to(x + w, y + h - br);
    if br > 0.0 {
        pb.cubic_to(
            x + w,
            y + h - br + br * k,
            x + w - br + br * k,
            y + h,
            x + w - br,
            y + h,
        );
    }

    // Bottom edge → BL corner.
    pb.line_to(x + bl, y + h);
    if bl > 0.0 {
        pb.cubic_to(
            x + bl - bl * k,
            y + h,
            x,
            y + h - bl + bl * k,
            x,
            y + h - bl,
        );
    }

    // Left edge → TL corner.
    pb.line_to(x, y + tl);
    if tl > 0.0 {
        pb.cubic_to(x, y + tl - tl * k, x + tl - tl * k, y, x + tl, y);
    }

    pb.close();
    pb.finish()
}

fn copy_rgba_to_bgra(dst: &mut [u8], src: &[u8]) {
    for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
        d[0] = s[2]; // B ← R
        d[1] = s[1]; // G ← G
        d[2] = s[0]; // R ← B
        d[3] = s[3]; // A ← A
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        font::load_ui_font, layout, render::scene::build_scene, wallpaper::scan_wallpapers,
    };
    use std::path::Path;

    /// Render visual a PNG: corré con
    ///   WALL_DIR=/tmp/bench_wallpapers cargo test --release render_dump_png -- --ignored --nocapture
    /// Genera /tmp/hyprwall_render.png con el picker contra el directorio.
    #[test]
    #[ignore]
    fn render_dump_png() {
        let dir = std::env::var("WALL_DIR").unwrap_or_else(|_| "/tmp/bench_wallpapers".into());
        let out = std::env::var("WALL_OUT").unwrap_or_else(|_| "/tmp/hyprwall_render.png".into());

        let wallpapers = scan_wallpapers(Path::new(&dir)).expect("scan_wallpapers");
        assert!(!wallpapers.is_empty(), "directorio sin wallpapers válidos");
        let font = load_ui_font().expect("font");

        let (w, h) = (1280u32, 560u32);
        let selected = 0;
        let layout = layout::compute(w, h, wallpapers.len(), selected);
        let scene = build_scene(w, h, &layout, &wallpapers, selected, Some(2));

        let mut canvas = vec![0u8; (w * h * 4) as usize];
        rasterize(&mut canvas, w, h, &scene, &font);

        // canvas viene BGRA premultiplied-ish. Para inspeccionar visualmente,
        // hacemos swap a RGBA y compositamos sobre fondo gris para que
        // visores que ignoran alpha lo muestren bien.
        let mut rgba = vec![0u8; canvas.len()];
        let bg = [40u8, 40, 50];
        for (d, s) in rgba.chunks_exact_mut(4).zip(canvas.chunks_exact(4)) {
            let a = s[3] as u16;
            let inv = 255 - a;
            d[0] = ((s[2] as u16 * a + bg[0] as u16 * inv) / 255) as u8; // R
            d[1] = ((s[1] as u16 * a + bg[1] as u16 * inv) / 255) as u8; // G
            d[2] = ((s[0] as u16 * a + bg[2] as u16 * inv) / 255) as u8; // B
            d[3] = 255;
        }

        let int_size = tiny_skia::IntSize::from_wh(w, h).unwrap();
        let pixmap = Pixmap::from_vec(rgba, int_size).expect("from_vec");
        pixmap.save_png(&out).expect("save_png");
        eprintln!("wrote {out}");
    }
}
