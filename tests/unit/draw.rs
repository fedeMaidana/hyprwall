use super::*;
use tiny_skia::{Mask, PixmapPaint};

fn thumbnail(width: u32, height: u32) -> Thumbnail {
    Thumbnail {
        width,
        height,
        rgba: (0..height)
            .flat_map(|y| {
                (0..width).flat_map(move |x| {
                    [
                        (x * 17 + y * 3) as u8,
                        (x * 5 + y * 19) as u8,
                        (x * 11 + y * 7) as u8,
                        255,
                    ]
                })
            })
            .collect(),
    }
}

fn background() -> Pixmap {
    let mut pixmap = Pixmap::new(180, 140).unwrap();
    pixmap.fill(tiny_skia::Color::from_rgba8(47, 63, 89, 173));
    pixmap
}

#[test]
fn flat_thumbnail_matches_masked_reference() {
    let mut translucent = thumbnail(84, 68);
    for pixel in translucent.rgba.as_chunks_mut::<4>().0.iter_mut() {
        let alpha = 48 + pixel[0] % 192;
        for channel in &mut pixel[..3] {
            *channel = (*channel as u16 * alpha as u16 / 255) as u8;
        }
        pixel[3] = alpha;
    }
    let thumbs = [
        thumbnail(84, 68),
        thumbnail(1, 1),
        thumbnail(7, 91),
        translucent,
    ];
    let corners = [
        Corners::ALL,
        Corners {
            top_left: false,
            top_right: true,
            bottom_left: true,
            bottom_right: false,
        },
    ];
    for thumb in &thumbs {
        for rect in [
            Rect {
                x: 33,
                y: 26,
                w: 61,
                h: 73,
            },
            Rect {
                x: -11,
                y: -7,
                w: 59,
                h: 95,
            },
            Rect {
                x: 151,
                y: 118,
                w: 62,
                h: 87,
            },
        ] {
            for corners in corners {
                for radius in [0, 9, 60] {
                    for opacity in [0.0, 0.3, 1.0] {
                        for shift in [-29, 0, 29] {
                            let mut expected = background();
                            let mut actual = expected.clone();
                            reference_flat(
                                &mut expected,
                                rect,
                                radius,
                                corners,
                                thumb,
                                shift,
                                opacity,
                            );
                            draw_thumbnail_flat(
                                &mut actual,
                                rect,
                                radius,
                                corners,
                                thumb,
                                shift,
                                opacity,
                            );
                            let max_delta = actual
                                .data()
                                .iter()
                                .zip(expected.data())
                                .map(|(a, b)| a.abs_diff(*b))
                                .max()
                                .unwrap();
                            assert!(
                                max_delta <= 1,
                                "flat max delta {max_delta}: rect={rect:?} radius={radius} opacity={opacity} shift={shift} thumb={}x{}",
                                thumb.width,
                                thumb.height
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn warped_thumbnail_matches_column_reference_exactly() {
    let thumbs = [
        thumbnail(84, 68),
        thumbnail(1, 1),
        thumbnail(1, 91),
        thumbnail(97, 1),
    ];
    let corners = [
        Corners::ALL,
        Corners {
            top_left: false,
            top_right: true,
            bottom_left: true,
            bottom_right: false,
        },
    ];
    for thumb in &thumbs {
        for rect in [
            Rect {
                x: 33,
                y: 26,
                w: 61,
                h: 73,
            },
            Rect {
                x: -11,
                y: -7,
                w: 59,
                h: 95,
            },
            Rect {
                x: 151,
                y: 118,
                w: 62,
                h: 87,
            },
            Rect {
                x: 211,
                y: 170,
                w: 41,
                h: 53,
            },
        ] {
            for corners in corners {
                for radius in [0, 9, 60] {
                    for opacity in [0.0, 0.3, 1.0] {
                        for tilt in [-1.0, -0.13, 0.0, 0.41, 1.0] {
                            let warp = CardWarp::new(
                                (rect.x + rect.w / 2) as f32,
                                (rect.y + rect.h / 2) as f32,
                                tilt,
                                rect.w as f32,
                            );
                            let mut expected = background();
                            let mut actual = expected.clone();
                            reference_warped(
                                &mut expected,
                                rect,
                                radius,
                                corners,
                                thumb,
                                -19,
                                opacity,
                                &warp,
                            );
                            draw_thumbnail_warped(
                                &mut actual,
                                rect,
                                radius,
                                corners,
                                thumb,
                                -19,
                                opacity,
                                &warp,
                            );
                            assert_eq!(
                                actual.data(),
                                expected.data(),
                                "warp rect={rect:?} radius={radius} opacity={opacity} tilt={tilt} thumb={}x{}",
                                thumb.width,
                                thumb.height
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn large_warped_thumbnail_matches_reference_across_tiles() {
    let thumb = thumbnail(624, 568);
    let rect = Rect {
        x: 253,
        y: 93,
        w: 352,
        h: 584,
    };
    for opacity in [0.4, 1.0] {
        for tilt in [-1.0, 0.43, 1.0] {
            let warp = CardWarp::new(429.0, 385.0, tilt, rect.w as f32);
            let mut expected = Pixmap::new(900, 800).unwrap();
            expected.fill(tiny_skia::Color::from_rgba8(47, 63, 89, 173));
            let mut actual = expected.clone();
            reference_warped(
                &mut expected,
                rect,
                32,
                Corners::ALL,
                &thumb,
                29,
                opacity,
                &warp,
            );
            draw_thumbnail_warped(
                &mut actual,
                rect,
                32,
                Corners::ALL,
                &thumb,
                29,
                opacity,
                &warp,
            );
            assert_eq!(
                actual.data(),
                expected.data(),
                "tiled warp opacity={opacity} tilt={tilt}"
            );
        }
    }
}

// Reference algorithms from before the optimization. Keeping the original
// traversal and clipping here catches changes in filtering, antialiasing,
// offscreen clipping, and compositing rather than just command structure.
fn reference_flat(
    pixmap: &mut Pixmap,
    rect: Rect,
    radius: i32,
    corners: Corners,
    thumb: &Thumbnail,
    shift_x: i32,
    opacity: f32,
) {
    let Some(thumb_ref) = PixmapRef::from_bytes(&thumb.rgba, thumb.width, thumb.height) else {
        return;
    };

    let (scale, dx, dy) = cover_fit(rect, thumb, shift_x);

    let transform = Transform::from_scale(scale, scale).post_translate(dx, dy);

    let mut mask = match Mask::new(pixmap.width(), pixmap.height()) {
        Some(m) => m,
        None => return,
    };
    let Some(clip) = round_rect_path(rect, radius, corners) else {
        return;
    };
    mask.fill_path(&clip, FillRule::Winding, true, Transform::identity());

    let paint = PixmapPaint {
        opacity: opacity.clamp(0.0, 1.0),
        quality: FilterQuality::Bilinear,
        ..PixmapPaint::default()
    };

    pixmap.draw_pixmap(0, 0, thumb_ref, &paint, transform, Some(&mask));
}

#[allow(clippy::too_many_arguments)]
fn reference_warped(
    pixmap: &mut Pixmap,
    rect: Rect,
    radius: i32,
    corners: Corners,
    thumb: &Thumbnail,
    shift_x: i32,
    opacity: f32,
    warp: &CardWarp,
) {
    let (scale, dx, dy) = cover_fit(rect, thumb, shift_x);

    let x0 = rect.x as f32;
    let x1 = (rect.x + rect.w) as f32;
    let y0 = rect.y as f32;
    let y1 = (rect.y + rect.h) as f32;

    let r_max = (rect.w.min(rect.h)) as f32 / 2.0;
    let r = (radius as f32).clamp(0.0, r_max.max(0.0));

    let width = pixmap.width() as i32;
    let height = pixmap.height() as i32;

    let (sx0, _) = warp.project(x0, y0);
    let (sx1, _) = warp.project(x1, y0);

    let col_start = (sx0.ceil() as i32).max(0);
    let col_end = (sx1.floor() as i32).min(width - 1);
    if col_end < col_start {
        return;
    }

    let tw = thumb.width as f32;
    let th = thumb.height as f32;
    let tx_max = (tw - 1.001).max(0.0);
    let ty_max = (th - 1.001).max(0.0);

    let alpha_mul = opacity.clamp(0.0, 1.0);
    let cy = warp.cy;

    let data = pixmap.data_mut();

    for col in col_start..=col_end {
        // Centro de la columna de pantalla → columna del plano de la card.
        let x = warp.invert_x(col as f32 + 0.5);
        let s = warp.column_scale(x).max(1e-3);
        let hx = warp.column_hscale(x).max(1e-3);

        // Anchos de anti-alias en unidades locales por px de pantalla.
        let aa_x = 1.0 / hx;
        let aa_y = 1.0 / s;

        // Extensión vertical proyectada de la card en esta columna.
        let top = cy + (y0 - cy) * s;
        let bot = cy + (y1 - cy) * s;

        let row_start = ((top - 1.0).floor() as i32).max(0);
        let row_end = ((bot + 1.0).ceil() as i32).min(height - 1);

        for row in row_start..=row_end {
            // Centro de la fila → y del plano de la card.
            let y = cy + (row as f32 + 0.5 - cy) / s;

            let cov = rounded_rect_coverage(x, y, x0, x1, y0, y1, r, corners, aa_x, aa_y);
            if cov <= 0.0 {
                continue;
            }

            let tx = ((x - dx) / scale).max(0.0).min(tx_max);
            let ty = ((y - dy) / scale).max(0.0).min(ty_max);
            let (sr, sg, sb) = bilinear_rgb(
                &thumb.rgba,
                thumb.width as usize,
                thumb.height as usize,
                tx,
                ty,
            );

            let a = cov * alpha_mul;
            let idx = ((row * width + col) * 4) as usize;
            blend_px(&mut data[idx..idx + 4], sr, sg, sb, a);
        }
    }
}

/// Cobertura [0, 1] de un punto local contra el rounded-rect de la card,
/// con anti-alias anisotrópico (aa_x/aa_y = tamaño del px de pantalla en
/// unidades locales).
#[allow(clippy::too_many_arguments)]
fn rounded_rect_coverage(
    x: f32,
    y: f32,
    x0: f32,
    x1: f32,
    y0: f32,
    y1: f32,
    r: f32,
    corners: Corners,
    aa_x: f32,
    aa_y: f32,
) -> f32 {
    let aa_x = aa_x.max(1e-3);
    let aa_y = aa_y.max(1e-3);

    let ex = (x - x0).min(x1 - x);
    let ey = (y - y0).min(y1 - y);

    let mut cov = (ex / aa_x + 0.5).clamp(0.0, 1.0) * (ey / aa_y + 0.5).clamp(0.0, 1.0);

    if cov > 0.0 && r > 0.0 {
        let in_left = x - x0 < r;
        let in_right = x1 - x < r;
        let in_top = y - y0 < r;
        let in_bot = y1 - y < r;

        let rounded = (in_left && in_top && corners.top_left)
            || (in_right && in_top && corners.top_right)
            || (in_right && in_bot && corners.bottom_right)
            || (in_left && in_bot && corners.bottom_left);

        if rounded {
            let cxr = if in_left { x0 + r } else { x1 - r };
            let cyr = if in_top { y0 + r } else { y1 - r };
            let vx = x - cxr;
            let vy = y - cyr;
            let len = (vx * vx + vy * vy).sqrt();
            if len > 1e-6 {
                let aa_c = (aa_x * (vx / len).abs() + aa_y * (vy / len).abs()).max(1e-3);
                let corner_cov = ((r - len) / aa_c + 0.5).clamp(0.0, 1.0);
                cov = cov.min(corner_cov);
            }
        }
    }

    cov
}

/// Muestra bilineal RGB (los wallpapers son opacos; el alpha lo pone la
/// cobertura).
fn bilinear_rgb(rgba: &[u8], width: usize, height: usize, x: f32, y: f32) -> (f32, f32, f32) {
    let xi = x as usize;
    let yi = y as usize;
    let x2 = (xi + 1).min(width.saturating_sub(1));
    let y2 = (yi + 1).min(height.saturating_sub(1));
    let fx = x - xi as f32;
    let fy = y - yi as f32;

    let at = |px: usize, py: usize| {
        let i = (py * width + px) * 4;
        (rgba[i] as f32, rgba[i + 1] as f32, rgba[i + 2] as f32)
    };

    let (r00, g00, b00) = at(xi, yi);
    let (r10, g10, b10) = at(x2, yi);
    let (r01, g01, b01) = at(xi, y2);
    let (r11, g11, b11) = at(x2, y2);

    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;

    (
        lerp(lerp(r00, r10, fx), lerp(r01, r11, fx), fy),
        lerp(lerp(g00, g10, fx), lerp(g01, g11, fx), fy),
        lerp(lerp(b00, b10, fx), lerp(b01, b11, fx), fy),
    )
}

/// Mezcla source-over premultiplicado de un pixel opaco con alpha `a`.
fn blend_px(dst: &mut [u8], r: f32, g: f32, b: f32, a: f32) {
    let a = a.clamp(0.0, 1.0);
    let inv = 1.0 - a;
    dst[0] = (r * a + dst[0] as f32 * inv + 0.5) as u8;
    dst[1] = (g * a + dst[1] as f32 * inv + 0.5) as u8;
    dst[2] = (b * a + dst[2] as f32 * inv + 0.5) as u8;
    dst[3] = (255.0 * a + dst[3] as f32 * inv + 0.5) as u8;
}
