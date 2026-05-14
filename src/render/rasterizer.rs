//! Scene rasterizer: walks a [`Scene`] and produces pixels.
//!
//! Responsibilities: HiDPI scaling of [`DrawCmd`] coordinates, the dispatch
//! loop over commands, and the final RGBA → BGRA swap for Wayland.
//! The actual drawing primitives (paths, gradients, thumbnails) live in
//! [`super::draw`].

use fontdue::Font;
use tiny_skia::Pixmap;

use crate::{
    geometry::Rect,
    render::{
        draw::{draw_thumbnail, fill_round_rect, fill_vertical_gradient},
        scene::{DrawCmd, Scene},
        text::draw_text_centered_in_rect,
    },
};

/// Walks a `Scene` in order, scales every command to physical pixels, paints
/// into an internal RGBA [`Pixmap`], then copies to `canvas` with an
/// RGBA → BGRA swap for Wayland's `Argb8888` format.
///
/// `width` / `height` are **physical** pixels; `scale` is the HiDPI factor.
pub fn rasterize(
    canvas: &mut [u8],
    width: u32,
    height: u32,
    scale: f32,
    scene: &Scene<'_>,
    font: &Font,
) {
    let Some(mut pixmap) = Pixmap::new(width, height) else {
        log::error!("no se pudo crear tiny-skia Pixmap {width}x{height}");
        return;
    };

    for cmd in &scene.commands {
        match scale_cmd(cmd, scale) {
            DrawCmd::RoundRect {
                rect,
                radius,
                corners,
                color,
            } => {
                fill_round_rect(&mut pixmap, rect, radius, corners, color);
            }
            DrawCmd::VerticalGradient {
                rect,
                radius,
                corners,
                bottom_color,
                top_alpha,
            } => {
                fill_vertical_gradient(&mut pixmap, rect, radius, corners, bottom_color, top_alpha);
            }
            DrawCmd::Thumbnail {
                rect,
                radius,
                corners,
                thumb,
            } => {
                draw_thumbnail(&mut pixmap, rect, radius, corners, thumb);
            }
            DrawCmd::Text {
                rect,
                text,
                font_size,
                color,
            } => {
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

    copy_rgba_to_bgra(canvas, pixmap.data());
}

fn scale_cmd<'a>(cmd: &DrawCmd<'a>, scale: f32) -> DrawCmd<'a> {
    match *cmd {
        DrawCmd::RoundRect {
            rect,
            radius,
            corners,
            color,
        } => DrawCmd::RoundRect {
            rect: scale_rect(rect, scale),
            radius: scale_len(radius, scale),
            corners,
            color,
        },
        DrawCmd::VerticalGradient {
            rect,
            radius,
            corners,
            bottom_color,
            top_alpha,
        } => DrawCmd::VerticalGradient {
            rect: scale_rect(rect, scale),
            radius: scale_len(radius, scale),
            corners,
            bottom_color,
            top_alpha,
        },
        DrawCmd::Thumbnail {
            rect,
            radius,
            corners,
            thumb,
        } => DrawCmd::Thumbnail {
            rect: scale_rect(rect, scale),
            radius: scale_len(radius, scale),
            corners,
            thumb,
        },
        DrawCmd::Text {
            rect,
            text,
            font_size,
            color,
        } => DrawCmd::Text {
            rect: scale_rect(rect, scale),
            text,
            font_size: font_size * scale,
            color,
        },
    }
}

fn scale_rect(r: Rect, scale: f32) -> Rect {
    Rect {
        x: (r.x as f32 * scale).round() as i32,
        y: (r.y as f32 * scale).round() as i32,
        w: (r.w as f32 * scale).round() as i32,
        h: (r.h as f32 * scale).round() as i32,
    }
}

fn scale_len(v: i32, scale: f32) -> i32 {
    (v as f32 * scale).round() as i32
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

    /// WALL_DIR=/tmp/bench_wallpapers cargo test --release render_dump_png -- --ignored --nocapture
    #[test]
    #[ignore]
    fn render_dump_png() {
        let dir = std::env::var("WALL_DIR").unwrap_or_else(|_| "/tmp/bench_wallpapers".into());
        let out = std::env::var("WALL_OUT").unwrap_or_else(|_| "/tmp/hyprwall_render.png".into());
        let scale: f32 = std::env::var("SCALE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1.0);

        let wallpapers = scan_wallpapers(Path::new(&dir)).expect("scan_wallpapers");
        assert!(!wallpapers.is_empty());
        let font = load_ui_font().expect("font");

        let (logical_w, logical_h) = (1280u32, 560u32);
        let phys_w = (logical_w as f32 * scale).round() as u32;
        let phys_h = (logical_h as f32 * scale).round() as u32;
        let layout = layout::compute(logical_w, logical_h, wallpapers.len(), 0);
        let scene = build_scene(logical_w, logical_h, &layout, &wallpapers, 0, Some(2));

        let mut canvas = vec![0u8; (phys_w * phys_h * 4) as usize];
        rasterize(&mut canvas, phys_w, phys_h, scale, &scene, &font);

        let mut rgba = vec![0u8; canvas.len()];
        let bg = [40u8, 40, 50];
        for (d, s) in rgba.chunks_exact_mut(4).zip(canvas.chunks_exact(4)) {
            let a = s[3] as u16;
            let inv = 255 - a;
            d[0] = ((s[2] as u16 * a + bg[0] as u16 * inv) / 255) as u8;
            d[1] = ((s[1] as u16 * a + bg[1] as u16 * inv) / 255) as u8;
            d[2] = ((s[0] as u16 * a + bg[2] as u16 * inv) / 255) as u8;
            d[3] = 255;
        }

        let int_size = tiny_skia::IntSize::from_wh(phys_w, phys_h).unwrap();
        let pixmap = Pixmap::from_vec(rgba, int_size).expect("from_vec");
        pixmap.save_png(&out).expect("save_png");
        eprintln!("wrote {out} ({phys_w}x{phys_h} @ scale={scale})");
    }
}
