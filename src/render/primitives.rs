use crate::{
    geometry::{Corners, Rect},
    style::Color,
    wallpaper::Thumbnail,
};

pub fn fill_round_rect_corners(
    canvas: &mut [u8],
    surface_w: u32,
    surface_h: u32,
    rect: Rect,
    radius: i32,
    corners: Corners,
    color: Color,
) {
    if radius <= 0 {
        fill_rect_fast(canvas, surface_w, surface_h, rect, color);
        return;
    }

    let x0 = rect.x.max(0);
    let y0 = rect.y.max(0);
    let x1 = (rect.x + rect.w).min(surface_w as i32);
    let y1 = (rect.y + rect.h).min(surface_h as i32);

    for y in y0..y1 {
        for x in x0..x1 {
            let coverage = rounded_rect_coverage(x, y, rect, radius, corners);
            if coverage == 0 {
                continue;
            }

            blend_pixel(
                canvas,
                surface_w,
                x,
                y,
                Color {
                    a: scale_alpha(color.a, coverage),
                    ..color
                },
            );
        }
    }
}

pub fn fill_vertical_gradient_round_rect_corners(
    canvas: &mut [u8],
    surface_w: u32,
    surface_h: u32,
    rect: Rect,
    radius: i32,
    corners: Corners,
    bottom_color: Color,
    top_alpha: u8,
) {
    let x0 = rect.x.max(0);
    let y0 = rect.y.max(0);
    let x1 = (rect.x + rect.w).min(surface_w as i32);
    let y1 = (rect.y + rect.h).min(surface_h as i32);

    if rect.h <= 0 {
        return;
    }

    for y in y0..y1 {
        let t = ((y - rect.y) as f32 / rect.h as f32).clamp(0.0, 1.0);

        // t=0 -> arriba
        // t=1 -> abajo
        let row_alpha =
            (top_alpha as f32 + (bottom_color.a as f32 - top_alpha as f32) * t).round() as u8;

        for x in x0..x1 {
            let coverage = rounded_rect_coverage(x, y, rect, radius, corners);
            if coverage == 0 {
                continue;
            }

            blend_pixel(
                canvas,
                surface_w,
                x,
                y,
                Color {
                    r: bottom_color.r,
                    g: bottom_color.g,
                    b: bottom_color.b,
                    a: scale_alpha(row_alpha, coverage),
                },
            );
        }
    }
}

fn fill_rect_fast(canvas: &mut [u8], surface_w: u32, surface_h: u32, rect: Rect, color: Color) {
    let x0 = rect.x.max(0);
    let y0 = rect.y.max(0);
    let x1 = (rect.x + rect.w).min(surface_w as i32);
    let y1 = (rect.y + rect.h).min(surface_h as i32);

    for y in y0..y1 {
        for x in x0..x1 {
            blend_pixel(canvas, surface_w, x, y, color);
        }
    }
}

pub fn draw_thumbnail(
    canvas: &mut [u8],
    surface_w: u32,
    surface_h: u32,
    rect: Rect,
    radius: i32,
    corners: Corners,
    thumb: &Thumbnail,
) {
    if rect.w <= 0 || rect.h <= 0 || thumb.width == 0 || thumb.height == 0 {
        return;
    }

    let scale_x = rect.w as f32 / thumb.width as f32;
    let scale_y = rect.h as f32 / thumb.height as f32;
    let scale = scale_x.max(scale_y);

    let scaled_w = thumb.width as f32 * scale;
    let scaled_h = thumb.height as f32 * scale;

    let crop_x = (scaled_w - rect.w as f32) / 2.0;
    let crop_y = (scaled_h - rect.h as f32) / 2.0;

    let x0 = rect.x.max(0);
    let y0 = rect.y.max(0);
    let x1 = (rect.x + rect.w).min(surface_w as i32);
    let y1 = (rect.y + rect.h).min(surface_h as i32);

    for y in y0..y1 {
        for x in x0..x1 {
            let coverage = rounded_rect_coverage(x, y, rect, radius, corners);
            if coverage == 0 {
                continue;
            }

            let local_x = x - rect.x;
            let local_y = y - rect.y;

            let src_x = ((local_x as f32 + crop_x) / scale)
                .floor()
                .clamp(0.0, thumb.width.saturating_sub(1) as f32) as u32;

            let src_y = ((local_y as f32 + crop_y) / scale)
                .floor()
                .clamp(0.0, thumb.height.saturating_sub(1) as f32) as u32;

            let src_idx = ((src_y * thumb.width + src_x) * 4) as usize;

            let color = Color {
                r: thumb.rgba[src_idx],
                g: thumb.rgba[src_idx + 1],
                b: thumb.rgba[src_idx + 2],
                a: scale_alpha(thumb.rgba[src_idx + 3], coverage),
            };

            blend_pixel(canvas, surface_w, x, y, color);
        }
    }
}

fn rounded_rect_coverage(x: i32, y: i32, rect: Rect, radius: i32, corners: Corners) -> u8 {
    if radius <= 0 {
        return 255;
    }

    if x < rect.x || y < rect.y || x >= rect.x + rect.w || y >= rect.y + rect.h {
        return 0;
    }

    let radius = radius.min(rect.w / 2).min(rect.h / 2);

    if radius <= 0 {
        return 255;
    }

    let left = rect.x;
    let top = rect.y;
    let right = rect.x + rect.w;
    let bottom = rect.y + rect.h;

    let is_top_left_corner = corners.top_left && x < left + radius && y < top + radius;
    let is_top_right_corner = corners.top_right && x >= right - radius && y < top + radius;
    let is_bottom_right_corner =
        corners.bottom_right && x >= right - radius && y >= bottom - radius;
    let is_bottom_left_corner = corners.bottom_left && x < left + radius && y >= bottom - radius;

    if !is_top_left_corner
        && !is_top_right_corner
        && !is_bottom_right_corner
        && !is_bottom_left_corner
    {
        return 255;
    }

    const STEPS: i32 = 4;
    let mut inside = 0;

    for sy in 0..STEPS {
        for sx in 0..STEPS {
            let px = x as f32 + (sx as f32 + 0.5) / STEPS as f32;
            let py = y as f32 + (sy as f32 + 0.5) / STEPS as f32;

            if point_inside_rounded_rect(px, py, rect, radius, corners) {
                inside += 1;
            }
        }
    }

    ((inside * 255) / (STEPS * STEPS)) as u8
}

fn point_inside_rounded_rect(px: f32, py: f32, rect: Rect, radius: i32, corners: Corners) -> bool {
    let left = rect.x as f32;
    let top = rect.y as f32;
    let right = (rect.x + rect.w) as f32;
    let bottom = (rect.y + rect.h) as f32;

    if px < left || px >= right || py < top || py >= bottom {
        return false;
    }

    if radius <= 0 {
        return true;
    }

    let r = radius as f32;
    let r2 = r * r;

    if corners.top_left && px < left + r && py < top + r {
        let dx = px - (left + r);
        let dy = py - (top + r);
        return dx * dx + dy * dy <= r2;
    }

    if corners.top_right && px >= right - r && py < top + r {
        let dx = px - (right - r);
        let dy = py - (top + r);
        return dx * dx + dy * dy <= r2;
    }

    if corners.bottom_right && px >= right - r && py >= bottom - r {
        let dx = px - (right - r);
        let dy = py - (bottom - r);
        return dx * dx + dy * dy <= r2;
    }

    if corners.bottom_left && px < left + r && py >= bottom - r {
        let dx = px - (left + r);
        let dy = py - (bottom - r);
        return dx * dx + dy * dy <= r2;
    }

    true
}

pub fn scale_alpha(alpha: u8, coverage: u8) -> u8 {
    ((alpha as u16 * coverage as u16) / 255) as u8
}

pub fn blend_pixel(canvas: &mut [u8], surface_w: u32, x: i32, y: i32, color: Color) {
    if color.a == 0 || x < 0 || y < 0 {
        return;
    }

    let idx = ((y as u32 * surface_w + x as u32) * 4) as usize;
    if idx + 3 >= canvas.len() {
        return;
    }

    let dst_b = canvas[idx] as u16;
    let dst_g = canvas[idx + 1] as u16;
    let dst_r = canvas[idx + 2] as u16;
    let dst_a = canvas[idx + 3] as u16;

    let a = color.a as u16;
    let inv_a = 255 - a;

    let out_b = (color.b as u16 * a + dst_b * inv_a) / 255;
    let out_g = (color.g as u16 * a + dst_g * inv_a) / 255;
    let out_r = (color.r as u16 * a + dst_r * inv_a) / 255;
    let out_a = a + (dst_a * inv_a) / 255;

    canvas[idx] = out_b as u8;
    canvas[idx + 1] = out_g as u8;
    canvas[idx + 2] = out_r as u8;
    canvas[idx + 3] = out_a as u8;
}
