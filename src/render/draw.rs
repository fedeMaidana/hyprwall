use std::sync::OnceLock;

use rayon::{ThreadPool, ThreadPoolBuilder, prelude::*};
use tiny_skia::{
    FillRule, FilterQuality, GradientStop, LinearGradient, Mask, Paint, Path, PathBuilder, Pattern,
    Pixmap, PixmapPaint, PixmapRef, Point, SpreadMode, Stroke, Transform,
};

use crate::{
    geometry::{Corners, Rect},
    style::{self, Color},
    wallpaper::Thumbnail,
};

const CIRCLE_KAPPA: f32 = 0.5522847498307933;

/// Proyección en perspectiva REAL de una card rotada alrededor del eje
/// vertical que pasa por su pivote. Coordenadas absolutas en px.
///
/// Modelo: un punto local (x, y) del plano de la card, rotado φ y visto
/// por una cámara a distancia `depth` con foco = depth:
///
///   z(x) = depth + x·sinφ          (profundidad de la columna x)
///   X    = cx + depth·x·cosφ/z(x)
///   Y    = cy + depth·y/z(x)
///
/// Propiedades: los bordes verticales siguen verticales, las líneas
/// horizontales del plano se proyectan como rectas, y la silueta de la
/// card es un trapecio exacto (borde cercano más alto). φ > 0 acerca el
/// borde IZQUIERDO: usar tilt > 0 para cards a la derecha del centro,
/// así ambos lados "miran" hacia adentro.
#[derive(Clone, Copy, Debug)]
pub struct CardWarp {
    cx: f32,
    cy: f32,
    sin_phi: f32,
    cos_phi: f32,
    depth: f32,
}

impl CardWarp {
    /// `tilt` ∈ [-1, 1]: fracción del ángulo máximo. `ref_width`: ancho
    /// de la card en px (la profundidad de cámara se expresa en anchos
    /// de card, así el efecto es independiente de la escala HiDPI).
    pub fn new(pivot_x: f32, pivot_y: f32, tilt: f32, ref_width: f32) -> Self {
        let phi = tilt.clamp(-1.0, 1.0) * style::card3d::ANGLE_DEG.to_radians();
        Self {
            cx: pivot_x,
            cy: pivot_y,
            sin_phi: phi.sin(),
            cos_phi: phi.cos(),
            depth: (style::card3d::DEPTH_CARDS * ref_width.abs()).max(1.0),
        }
    }

    /// Profundidad de la columna local x. Con las constantes sanas nunca
    /// se acerca a 0; el clamp es un cinturón de seguridad.
    fn z(&self, local_x: f32) -> f32 {
        (self.depth + local_x * self.sin_phi).max(self.depth * 0.05)
    }

    /// Punto absoluto del plano de la card → punto de pantalla.
    pub(super) fn project(&self, x: f32, y: f32) -> (f32, f32) {
        let lx = x - self.cx;
        let z = self.z(lx);
        (
            self.cx + self.depth * lx * self.cos_phi / z,
            self.cy + self.depth * (y - self.cy) / z,
        )
    }

    /// Escala vertical de la columna del x absoluto dado (1.0 en el pivote).
    fn column_scale(&self, x: f32) -> f32 {
        self.depth / self.z(x - self.cx)
    }

    /// Escala horizontal local dX/dx en la columna del x absoluto dado.
    fn column_hscale(&self, x: f32) -> f32 {
        let z = self.z(x - self.cx);
        self.depth * self.depth * self.cos_phi.max(0.05) / (z * z)
    }

    /// Columna de pantalla → x absoluto en el plano de la card (inversa
    /// exacta de la componente X de `project`).
    fn invert_x(&self, screen_x: f32) -> f32 {
        let sx = screen_x - self.cx;
        let denom = (self.depth * self.cos_phi - sx * self.sin_phi).max(self.depth * 0.05);
        self.cx + sx * self.depth / denom
    }
}

/// Path de la card: rounded-rect plano, o trapecio redondeado si hay warp.
fn card_path(rect: Rect, radius: i32, corners: Corners, warp: Option<&CardWarp>) -> Option<Path> {
    match warp {
        Some(w) => warped_round_rect_path(rect, radius, corners, w),
        None => round_rect_path(rect, radius, corners),
    }
}

pub fn fill_round_rect(
    pixmap: &mut Pixmap,
    rect: Rect,
    radius: i32,
    corners: Corners,
    color: Color,
    warp: Option<&CardWarp>,
) {
    let Some(path) = card_path(rect, radius, corners, warp) else {
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

pub fn stroke_round_rect(
    pixmap: &mut Pixmap,
    rect: Rect,
    radius: i32,
    corners: Corners,
    width: f32,
    color: Color,
    warp: Option<&CardWarp>,
) {
    let Some(path) = card_path(rect, radius, corners, warp) else {
        return;
    };

    let mut paint = Paint::default();
    paint.set_color_rgba8(color.r, color.g, color.b, color.a);
    paint.anti_alias = true;

    let stroke = Stroke {
        width,
        ..Stroke::default()
    };

    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
}

pub fn fill_vertical_gradient(
    pixmap: &mut Pixmap,
    rect: Rect,
    radius: i32,
    corners: Corners,
    bottom_color: Color,
    top_alpha: u8,
    warp: Option<&CardWarp>,
) {
    if rect.h <= 0 {
        return;
    }

    let Some(path) = card_path(rect, radius, corners, warp) else {
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

    // Extremos del gradiente proyectados por la columna central de la
    // card, para que el fundido acompañe la inclinación.
    let mid_x = rect.x as f32 + rect.w as f32 / 2.0;
    let y0 = rect.y as f32;
    let y1 = (rect.y + rect.h) as f32;
    let (gx, gy0, gy1) = match warp {
        Some(w) => {
            let (px, py0) = w.project(mid_x, y0);
            let (_, py1) = w.project(mid_x, y1);
            (px, py0, py1)
        }
        None => (mid_x, y0, y1),
    };

    let Some(shader) = LinearGradient::new(
        Point::from_xy(gx, gy0),
        Point::from_xy(gx, gy1),
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

pub fn draw_thumbnail(
    pixmap: &mut Pixmap,
    rect: Rect,
    radius: i32,
    corners: Corners,
    thumb: &Thumbnail,
    shift_x: i32,
    opacity: f32,
    warp: Option<&CardWarp>,
) {
    if rect.w <= 0 || rect.h <= 0 || thumb.width == 0 || thumb.height == 0 {
        return;
    }

    match warp {
        Some(w) => draw_thumbnail_warped(pixmap, rect, radius, corners, thumb, shift_x, opacity, w),
        None => draw_thumbnail_flat(pixmap, rect, radius, corners, thumb, shift_x, opacity),
    }
}

/// La card central opaca se pinta con un patrón dentro de su silueta,
/// evitando construir una máscara del tamaño de toda la pantalla.
fn draw_thumbnail_flat(
    pixmap: &mut Pixmap,
    rect: Rect,
    radius: i32,
    corners: Corners,
    thumb: &Thumbnail,
    shift_x: i32,
    opacity: f32,
) {
    if opacity <= 0.0 {
        return;
    }
    let Some(thumb_ref) = PixmapRef::from_bytes(&thumb.rgba, thumb.width, thumb.height) else {
        return;
    };

    let (scale, dx, dy) = cover_fit(rect, thumb, shift_x);

    let transform = Transform::from_scale(scale, scale).post_translate(dx, dy);

    let Some(clip) = round_rect_path(rect, radius, corners) else {
        return;
    };
    if opacity < 1.0 {
        // tiny-skia comparte la cobertura del antialias con la opacidad de
        // Pattern: para transparencias hay que mantener la máscara separada.
        // La card central normalmente es opaca; las laterales usan el warp.
        let Some(mut mask) = Mask::new(pixmap.width(), pixmap.height()) else {
            return;
        };
        mask.fill_path(&clip, FillRule::Winding, true, Transform::identity());
        let paint = PixmapPaint {
            opacity,
            quality: FilterQuality::Bilinear,
            ..PixmapPaint::default()
        };
        pixmap.draw_pixmap(0, 0, thumb_ref, &paint, transform, Some(&mask));
        return;
    }
    let paint = Paint {
        shader: Pattern::new(
            thumb_ref,
            SpreadMode::Pad,
            FilterQuality::Bilinear,
            opacity.clamp(0.0, 1.0),
            transform,
        ),
        anti_alias: true,
        ..Paint::default()
    };

    pixmap.fill_path(
        &clip,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

/// Escala y origen del cover-fit + paneo parallax recortado al sobrante.
fn cover_fit(rect: Rect, thumb: &Thumbnail, shift_x: i32) -> (f32, f32, f32) {
    let scale_x = rect.w as f32 / thumb.width as f32;
    let scale_y = rect.h as f32 / thumb.height as f32;
    let scale = scale_x.max(scale_y);

    let scaled_w = thumb.width as f32 * scale;
    let scaled_h = thumb.height as f32 * scale;

    let slack = ((scaled_w - rect.w as f32) / 2.0).max(0.0);
    let shift = (shift_x as f32).clamp(-slack, slack);

    let dx = rect.x as f32 + (rect.w as f32 - scaled_w) / 2.0 + shift;
    let dy = rect.y as f32 + (rect.h as f32 - scaled_h) / 2.0;

    (scale, dx, dy)
}

/// Imagen en perspectiva: precalcula la inversa y el muestreo horizontal
/// de cada columna, luego escribe filas contiguas del framebuffer. Conserva
/// el bilineal y el anti-alias anisotrópico del rounded-rect original.
#[allow(clippy::too_many_arguments)]
fn draw_thumbnail_warped(
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

    // Estos valores dependen de la columna, no del píxel. En particular,
    // no hay que invertir la proyección ni buscar vecinos X por cada fila.
    let columns: Vec<_> = (col_start..=col_end)
        .map(|col| {
            let x = warp.invert_x(col as f32 + 0.5);
            let s = warp.column_scale(x).max(1e-3);
            let hx = warp.column_hscale(x).max(1e-3);
            let aa_x = (1.0 / hx).max(1e-3);
            let aa_y = (1.0 / s).max(1e-3);
            let top = cy + (y0 - cy) * s;
            let bot = cy + (y1 - cy) * s;
            let tx = ((x - dx) / scale).max(0.0).min(tx_max);
            let xi = tx as usize;
            let in_left = x - x0 < r;
            let in_right = x1 - x < r;

            WarpColumn {
                scale_y: s,
                row_start: ((top - 1.0).floor() as i32).max(0),
                row_end: ((bot + 1.0).ceil() as i32).min(height - 1),
                texture_x: xi * 4,
                texture_x2: (xi + 1).min(thumb.width as usize - 1) * 4,
                texture_weight: tx - xi as f32,
                coverage_x: ((x - x0).min(x1 - x) / aa_x + 0.5).clamp(0.0, 1.0),
                aa_x,
                aa_y,
                corner_x: x - if in_left { x0 + r } else { x1 - r },
                round_top: r > 0.0
                    && ((in_left && corners.top_left) || (in_right && corners.top_right)),
                round_bottom: r > 0.0
                    && ((in_left && corners.bottom_left) || (in_right && corners.bottom_right)),
            }
        })
        .collect();

    let row_start = columns.iter().map(|c| c.row_start).min().unwrap();
    let row_end = columns.iter().map(|c| c.row_end).max().unwrap();
    if row_end < row_start {
        return;
    }
    let stride = width as usize * 4;
    let texture_stride = thumb.width as usize * 4;
    let rows = &mut pixmap.data_mut()[row_start as usize * stride..(row_end as usize + 1) * stride];
    let tile_bytes = stride * THUMBNAIL_TILE_ROWS;

    let render_tile = |(tile_index, tile): (usize, &mut [u8])| {
        for (local_row, dst_row) in tile.chunks_exact_mut(stride).enumerate() {
            let row = row_start + (tile_index * THUMBNAIL_TILE_ROWS + local_row) as i32;
            let start = col_start as usize * 4;
            let dst = &mut dst_row[start..start + columns.len() * 4];
            for (pixel, column) in dst.as_chunks_mut::<4>().0.iter_mut().zip(&columns) {
                if row < column.row_start || row > column.row_end {
                    continue;
                }
                let y = cy + (row as f32 + 0.5 - cy) / column.scale_y;
                let cov = column.coverage(y, y0, y1, r);
                if cov <= 0.0 {
                    continue;
                }

                let ty = ((y - dy) / scale).max(0.0).min(ty_max);
                let yi = ty as usize;
                let texture_row = yi * texture_stride;
                let texture_row2 = (yi + 1).min(thumb.height as usize - 1) * texture_stride;
                let (sr, sg, sb) =
                    column.sample(&thumb.rgba, texture_row, texture_row2, ty - yi as f32);
                blend_px(pixel, sr, sg, sb, cov * alpha_mul);
            }
        }
    };

    // Un tile tiene filas exclusivas: no cambia el orden de composición
    // entre cards ni el resultado de ningún píxel. Miniaturas pequeñas
    // evitan por completo el coste de despachar trabajo a otros hilos.
    let pixel_count = (row_end - row_start + 1) as usize * columns.len();
    if pixel_count >= PARALLEL_THUMBNAIL_PIXELS {
        if let Some(pool) = thumbnail_pool() {
            pool.install(|| {
                rows.par_chunks_mut(tile_bytes)
                    .enumerate()
                    .for_each(render_tile)
            });
            return;
        }
    }
    rows.chunks_mut(tile_bytes)
        .enumerate()
        .for_each(render_tile);
}

// Limitar el pool evita despertar todos los núcleos del sistema para cada
// card; también lo separa de la decodificación inicial que usa Rayon global.
const THUMBNAIL_TILE_ROWS: usize = 32;
const PARALLEL_THUMBNAIL_PIXELS: usize = 128 * 1024;

fn thumbnail_pool() -> Option<&'static ThreadPool> {
    static POOL: OnceLock<Option<ThreadPool>> = OnceLock::new();
    POOL.get_or_init(|| {
        let threads = std::thread::available_parallelism()
            .map_or(1, usize::from)
            .min(4);
        if threads <= 1 {
            return None;
        }
        ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|index| format!("hyprwall-render-{index}"))
            .build()
            .ok()
    })
    .as_ref()
}

struct WarpColumn {
    scale_y: f32,
    row_start: i32,
    row_end: i32,
    texture_x: usize,
    texture_x2: usize,
    texture_weight: f32,
    coverage_x: f32,
    aa_x: f32,
    aa_y: f32,
    corner_x: f32,
    round_top: bool,
    round_bottom: bool,
}

impl WarpColumn {
    fn coverage(&self, y: f32, y0: f32, y1: f32, radius: f32) -> f32 {
        let ey = (y - y0).min(y1 - y);
        let mut cov = self.coverage_x * (ey / self.aa_y + 0.5).clamp(0.0, 1.0);
        let in_top = y - y0 < radius;
        let in_bottom = y1 - y < radius;
        if cov > 0.0 && ((self.round_top && in_top) || (self.round_bottom && in_bottom)) {
            let vy = y - if in_top { y0 + radius } else { y1 - radius };
            let vx = self.corner_x;
            let len = (vx * vx + vy * vy).sqrt();
            if len > 1e-6 {
                let aa = (self.aa_x * (vx / len).abs() + self.aa_y * (vy / len).abs()).max(1e-3);
                cov = cov.min(((radius - len) / aa + 0.5).clamp(0.0, 1.0));
            }
        }
        cov
    }

    fn sample(&self, rgba: &[u8], row: usize, row2: usize, fy: f32) -> (f32, f32, f32) {
        let at = |offset: usize| {
            (
                rgba[offset] as f32,
                rgba[offset + 1] as f32,
                rgba[offset + 2] as f32,
            )
        };
        let (r00, g00, b00) = at(row + self.texture_x);
        let (r10, g10, b10) = at(row + self.texture_x2);
        let (r01, g01, b01) = at(row2 + self.texture_x);
        let (r11, g11, b11) = at(row2 + self.texture_x2);
        let fx = self.texture_weight;
        let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
        (
            lerp(lerp(r00, r10, fx), lerp(r01, r11, fx), fy),
            lerp(lerp(g00, g10, fx), lerp(g01, g11, fx), fy),
            lerp(lerp(b00, b10, fx), lerp(b01, b11, fx), fy),
        )
    }
}

/// Mezcla source-over premultiplicado de un pixel opaco con alpha `a`.
fn blend_px(dst: &mut [u8], r: f32, g: f32, b: f32, a: f32) {
    if a >= 1.0 {
        dst[0] = (r + 0.5) as u8;
        dst[1] = (g + 0.5) as u8;
        dst[2] = (b + 0.5) as u8;
        dst[3] = 255;
        return;
    }
    let a = a.clamp(0.0, 1.0);
    let inv = 1.0 - a;
    dst[0] = (r * a + dst[0] as f32 * inv + 0.5) as u8;
    dst[1] = (g * a + dst[1] as f32 * inv + 0.5) as u8;
    dst[2] = (b * a + dst[2] as f32 * inv + 0.5) as u8;
    dst[3] = (255.0 * a + dst[3] as f32 * inv + 0.5) as u8;
}

/// Trapecio redondeado: el rounded-rect de la card proyectado por el
/// warp. Los 4 vértices se proyectan exacto (bordes verticales siguen
/// verticales, horizontales quedan rectas) y los radios de las esquinas
/// se escalan con la profundidad de su borde.
fn warped_round_rect_path(
    rect: Rect,
    radius: i32,
    corners: Corners,
    warp: &CardWarp,
) -> Option<Path> {
    if rect.w <= 0 || rect.h <= 0 {
        return None;
    }

    let r_max = (rect.w.min(rect.h)) / 2;
    let r = radius.clamp(0, r_max) as f32;

    let x0 = rect.x as f32;
    let x1 = (rect.x + rect.w) as f32;
    let y0 = rect.y as f32;
    let y1 = (rect.y + rect.h) as f32;

    let tl = warp.project(x0, y0);
    let tr = warp.project(x1, y0);
    let br = warp.project(x1, y1);
    let bl = warp.project(x0, y1);

    // Radios por borde: verticales escalan con la altura de la columna,
    // horizontales con la compresión local.
    let rv_l = r * warp.column_scale(x0);
    let rv_r = r * warp.column_scale(x1);
    let rh_l = r * warp.column_hscale(x0);
    let rh_r = r * warp.column_hscale(x1);

    // Direcciones de viaje por los bordes superior (izq→der) e inferior
    // (der→izq); los laterales son verticales exactos.
    let top_dir = norm(tr.0 - tl.0, tr.1 - tl.1);
    let bot_dir = norm(bl.0 - br.0, bl.1 - br.1);

    let (tl_h, tl_v) = if corners.top_left {
        (rh_l, rv_l)
    } else {
        (0.0, 0.0)
    };
    let (tr_h, tr_v) = if corners.top_right {
        (rh_r, rv_r)
    } else {
        (0.0, 0.0)
    };
    let (br_h, br_v) = if corners.bottom_right {
        (rh_r, rv_r)
    } else {
        (0.0, 0.0)
    };
    let (bl_h, bl_v) = if corners.bottom_left {
        (rh_l, rv_l)
    } else {
        (0.0, 0.0)
    };

    let k = CIRCLE_KAPPA;
    let mut pb = PathBuilder::new();

    // Arranque sobre el borde superior, pasada la esquina TL.
    let start = (tl.0 + top_dir.0 * tl_h, tl.1 + top_dir.1 * tl_h);
    pb.move_to(start.0, start.1);

    // Borde superior → esquina TR.
    let a = (tr.0 - top_dir.0 * tr_h, tr.1 - top_dir.1 * tr_h);
    pb.line_to(a.0, a.1);
    if tr_h > 0.0 {
        let end = (tr.0, tr.1 + tr_v);
        pb.cubic_to(
            a.0 + top_dir.0 * tr_h * k,
            a.1 + top_dir.1 * tr_h * k,
            end.0,
            end.1 - tr_v * k,
            end.0,
            end.1,
        );
    }

    // Borde derecho → esquina BR.
    let a = (br.0, br.1 - br_v);
    pb.line_to(a.0, a.1);
    if br_v > 0.0 {
        let end = (br.0 + bot_dir.0 * br_h, br.1 + bot_dir.1 * br_h);
        pb.cubic_to(
            a.0,
            a.1 + br_v * k,
            end.0 - bot_dir.0 * br_h * k,
            end.1 - bot_dir.1 * br_h * k,
            end.0,
            end.1,
        );
    }

    // Borde inferior → esquina BL.
    let a = (bl.0 - bot_dir.0 * bl_h, bl.1 - bot_dir.1 * bl_h);
    pb.line_to(a.0, a.1);
    if bl_h > 0.0 {
        let end = (bl.0, bl.1 - bl_v);
        pb.cubic_to(
            a.0 + bot_dir.0 * bl_h * k,
            a.1 + bot_dir.1 * bl_h * k,
            end.0,
            end.1 + bl_v * k,
            end.0,
            end.1,
        );
    }

    // Borde izquierdo → esquina TL, de vuelta al arranque.
    let a = (tl.0, tl.1 + tl_v);
    pb.line_to(a.0, a.1);
    if tl_v > 0.0 {
        pb.cubic_to(
            a.0,
            a.1 - tl_v * k,
            start.0 - top_dir.0 * tl_h * k,
            start.1 - top_dir.1 * tl_h * k,
            start.0,
            start.1,
        );
    }

    pb.close();
    pb.finish()
}

fn norm(x: f32, y: f32) -> (f32, f32) {
    let len = (x * x + y * y).sqrt().max(1e-6);
    (x / len, y / len)
}

pub fn round_rect_path(rect: Rect, radius: i32, corners: Corners) -> Option<Path> {
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

    pb.move_to(x + tl, y);

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

    pb.line_to(x, y + tl);
    if tl > 0.0 {
        pb.cubic_to(x, y + tl - tl * k, x + tl - tl * k, y, x + tl, y);
    }

    pb.close();
    pb.finish()
}

#[cfg(test)]
#[path = "../../tests/unit/draw.rs"]
mod tests;
