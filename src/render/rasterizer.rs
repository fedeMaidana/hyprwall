//! Scene rasterizer: walks a [`Scene`] and produces pixels.

use fontdue::Font;
use tiny_skia::Pixmap;

use crate::{
    geometry::Rect,
    render::{
        draw::{draw_thumbnail, fill_round_rect, fill_vertical_gradient, stroke_round_rect},
        scene::{DrawCmd, Scene},
        text::draw_text_centered_in_rect,
    },
};

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
            } => fill_round_rect(&mut pixmap, rect, radius, corners, color),
            DrawCmd::StrokeRoundRect {
                rect,
                radius,
                corners,
                width,
                color,
            } => stroke_round_rect(&mut pixmap, rect, radius, corners, width, color),
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
        DrawCmd::StrokeRoundRect {
            rect,
            radius,
            corners,
            width,
            color,
        } => DrawCmd::StrokeRoundRect {
            rect: scale_rect(rect, scale),
            radius: scale_len(radius, scale),
            corners,
            width: width * scale,
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
        d[0] = s[2];
        d[1] = s[1];
        d[2] = s[0];
        d[3] = s[3];
    }
}

#[cfg(test)]
#[path = "../../tests/unit/rasterizer.rs"]
mod tests;
