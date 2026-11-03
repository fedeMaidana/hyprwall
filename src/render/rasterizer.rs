//! Scene rasterizer: walks a [`Scene`] and produces pixels.

use fontdue::Font;
use tiny_skia::Pixmap;

use crate::{
    geometry::Rect,
    render::{
        draw::{
            CardWarp, draw_thumbnail, fill_round_rect, fill_vertical_gradient, stroke_round_rect,
        },
        scene::{Background, DrawCmd, Scene},
        text::{draw_text_centered_in_rect, text_bounds},
    },
};

/// Retains the static background and the most recent complete frame.
/// Dynamic damage grows within a cache generation: even a SHM buffer that
/// skipped several frames can be updated using just this region.
#[derive(Default)]
pub struct Rasterizer {
    cache: Option<FrameCache>,
    generation: u64,
}

struct FrameCache {
    background_key: Background,
    scale: f32,
    background: Pixmap,
    pixmap: Pixmap,
    bgra: Vec<u8>,
    damage: Rect,
}

pub struct RenderedFrame<'a> {
    pixels: &'a [u8],
    width: u32,
    pub damage: Rect,
    pub generation: u64,
}

impl RenderedFrame<'_> {
    /// New buffers need the entire image, including the static backdrop.
    /// Reused buffers in this generation only need the accumulated damage.
    pub fn copy_to(&self, canvas: &mut [u8], initialize: bool) {
        if initialize {
            canvas[..self.pixels.len()].copy_from_slice(self.pixels);
        } else {
            copy_rect(canvas, self.pixels, self.width, self.damage);
        }
    }
}

impl Rasterizer {
    pub fn render(
        &mut self,
        width: u32,
        height: u32,
        scale: f32,
        scene: &Scene<'_>,
        font: &Font,
    ) -> Option<RenderedFrame<'_>> {
        let rebuild = self.cache.as_ref().is_none_or(|cache| {
            cache.pixmap.width() != width
                || cache.pixmap.height() != height
                || cache.scale != scale
                || cache.background_key != scene.background
        });
        if rebuild {
            let mut background = Pixmap::new(width, height)?;
            draw_commands(
                &mut background,
                scale,
                &scene.commands[..scene.background_len],
                font,
            );
            let pixmap = background.clone();
            let mut bgra = vec![0; background.data().len()];
            copy_rgba_to_bgra(&mut bgra, background.data());
            self.cache = Some(FrameCache {
                background_key: scene.background,
                scale,
                background,
                pixmap,
                bgra,
                damage: Rect {
                    x: 0,
                    y: 0,
                    w: 0,
                    h: 0,
                },
            });
            self.generation = self.generation.wrapping_add(1);
        }

        let cache = self.cache.as_mut()?;
        let commands = &scene.commands[scene.background_len..];
        let bounds = command_bounds(commands, scale, width, height, font);
        cache.damage = union_rect(cache.damage, bounds);
        // Restore pixels under both the old and new card positions, preserving
        // the backdrop outside the carousel without clearing or repainting it.
        copy_rect(
            cache.pixmap.data_mut(),
            cache.background.data(),
            width,
            cache.damage,
        );
        draw_commands(&mut cache.pixmap, scale, commands, font);
        convert_rect(&mut cache.bgra, cache.pixmap.data(), width, cache.damage);
        Some(RenderedFrame {
            pixels: &cache.bgra,
            width,
            damage: cache.damage,
            generation: self.generation,
        })
    }
}

fn draw_commands(pixmap: &mut Pixmap, scale: f32, commands: &[DrawCmd<'_>], font: &Font) {
    let width = pixmap.width();
    let height = pixmap.height();
    // Proyección vigente del grupo BeginTilt/EndTilt (rotación 3D en
    // perspectiva de las cards laterales). None fuera de los grupos.
    let mut warp: Option<CardWarp> = None;

    for cmd in commands {
        match scale_cmd(cmd, scale) {
            DrawCmd::BeginTilt {
                pivot_x,
                pivot_y,
                tilt,
                ref_width,
            } => {
                warp = Some(CardWarp::new(pivot_x, pivot_y, tilt, ref_width));
            }
            DrawCmd::EndTilt => warp = None,
            DrawCmd::RoundRect {
                rect,
                radius,
                corners,
                color,
            } => fill_round_rect(pixmap, rect, radius, corners, color, warp.as_ref()),
            DrawCmd::StrokeRoundRect {
                rect,
                radius,
                corners,
                width,
                color,
            } => stroke_round_rect(pixmap, rect, radius, corners, width, color, warp.as_ref()),
            DrawCmd::VerticalGradient {
                rect,
                radius,
                corners,
                bottom_color,
                top_alpha,
            } => fill_vertical_gradient(
                pixmap,
                rect,
                radius,
                corners,
                bottom_color,
                top_alpha,
                warp.as_ref(),
            ),
            DrawCmd::Thumbnail {
                rect,
                radius,
                corners,
                thumb,
                shift_x,
                opacity,
            } => draw_thumbnail(
                pixmap,
                rect,
                radius,
                corners,
                thumb,
                shift_x,
                opacity,
                warp.as_ref(),
            ),
            // El texto se dibuja directo al buffer (sin paths): queda sin
            // inclinar a propósito — etiquetas siempre derechas.
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
}

/// Full redraw reference used by pixel regression tests and PNG dumps.
#[cfg(test)]
pub fn rasterize(
    canvas: &mut [u8],
    width: u32,
    height: u32,
    scale: f32,
    scene: &Scene<'_>,
    font: &Font,
) {
    let mut pixmap = Pixmap::new(width, height).expect("valid canvas size");
    draw_commands(&mut pixmap, scale, &scene.commands, font);
    copy_rgba_to_bgra(canvas, pixmap.data());
}

fn command_bounds(
    commands: &[DrawCmd<'_>],
    scale: f32,
    width: u32,
    height: u32,
    font: &Font,
) -> Rect {
    let mut bounds = Rect {
        x: 0,
        y: 0,
        w: 0,
        h: 0,
    };
    let mut warp = None;
    for cmd in commands {
        let (rect, padding, projected) = match scale_cmd(cmd, scale) {
            DrawCmd::BeginTilt {
                pivot_x,
                pivot_y,
                tilt,
                ref_width,
            } => {
                warp = Some(CardWarp::new(pivot_x, pivot_y, tilt, ref_width));
                continue;
            }
            DrawCmd::EndTilt => {
                warp = None;
                continue;
            }
            DrawCmd::Text {
                rect,
                text,
                font_size,
                ..
            } => {
                let Some(rect) = text_bounds(font, text, font_size, rect) else {
                    continue;
                };
                (rect, 1.0, false)
            }
            DrawCmd::StrokeRoundRect { rect, width, .. } => (rect, width / 2.0 + 2.0, true),
            DrawCmd::RoundRect { rect, .. }
            | DrawCmd::VerticalGradient { rect, .. }
            | DrawCmd::Thumbnail { rect, .. } => (rect, 2.0, true),
        };
        if rect.w <= 0 || rect.h <= 0 {
            continue;
        }
        let mut x0 = f32::INFINITY;
        let mut y0 = f32::INFINITY;
        let mut x1 = f32::NEG_INFINITY;
        let mut y1 = f32::NEG_INFINITY;
        for (x, y) in [
            (rect.x, rect.y),
            (rect.x + rect.w, rect.y),
            (rect.x, rect.y + rect.h),
            (rect.x + rect.w, rect.y + rect.h),
        ] {
            let (x, y) = if projected {
                warp.as_ref()
                    .map_or((x as f32, y as f32), |w| w.project(x as f32, y as f32))
            } else {
                (x as f32, y as f32)
            };
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
        let x0 = ((x0 - padding).floor() as i32).clamp(0, width as i32);
        let y0 = ((y0 - padding).floor() as i32).clamp(0, height as i32);
        let x1 = ((x1 + padding).ceil() as i32).clamp(0, width as i32);
        let y1 = ((y1 + padding).ceil() as i32).clamp(0, height as i32);
        bounds = union_rect(
            bounds,
            Rect {
                x: x0,
                y: y0,
                w: (x1 - x0).max(0),
                h: (y1 - y0).max(0),
            },
        );
    }
    bounds
}

fn union_rect(a: Rect, b: Rect) -> Rect {
    if a.w <= 0 || a.h <= 0 {
        return b;
    }
    if b.w <= 0 || b.h <= 0 {
        return a;
    }
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    Rect {
        x,
        y,
        w: (a.x + a.w).max(b.x + b.w) - x,
        h: (a.y + a.h).max(b.y + b.h) - y,
    }
}

fn copy_rect(dst: &mut [u8], src: &[u8], width: u32, rect: Rect) {
    let stride = width as usize * 4;
    let row_len = rect.w as usize * 4;
    for y in rect.y..rect.y + rect.h {
        let offset = y as usize * stride + rect.x as usize * 4;
        dst[offset..offset + row_len].copy_from_slice(&src[offset..offset + row_len]);
    }
}

fn convert_rect(dst: &mut [u8], src: &[u8], width: u32, rect: Rect) {
    let stride = width as usize * 4;
    let row_len = rect.w as usize * 4;
    for y in rect.y..rect.y + rect.h {
        let offset = y as usize * stride + rect.x as usize * 4;
        copy_rgba_to_bgra(
            &mut dst[offset..offset + row_len],
            &src[offset..offset + row_len],
        );
    }
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
            shift_x,
            opacity,
        } => DrawCmd::Thumbnail {
            rect: scale_rect(rect, scale),
            radius: scale_len(radius, scale),
            corners,
            thumb,
            shift_x: scale_len(shift_x, scale),
            opacity,
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
        DrawCmd::BeginTilt {
            pivot_x,
            pivot_y,
            tilt,
            ref_width,
        } => DrawCmd::BeginTilt {
            pivot_x: pivot_x * scale,
            pivot_y: pivot_y * scale,
            // tilt es adimensional; ref_width es una longitud y escala.
            tilt,
            ref_width: ref_width * scale,
        },
        DrawCmd::EndTilt => DrawCmd::EndTilt,
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
    for (d, s) in dst
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(src.as_chunks::<4>().0.iter())
    {
        d[0] = s[2];
        d[1] = s[1];
        d[2] = s[0];
        d[3] = s[3];
    }
}

#[cfg(test)]
#[path = "../../tests/unit/rasterizer.rs"]
mod tests;
