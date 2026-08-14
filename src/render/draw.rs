use tiny_skia::{
    FillRule, FilterQuality, GradientStop, LinearGradient, Mask, Paint, Path, PathBuilder, Pixmap,
    PixmapPaint, PixmapRef, Point, SpreadMode, Stroke, Transform,
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
    fn project(&self, x: f32, y: f32) -> (f32, f32) {
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

/// Camino plano original: cover-fit + parallax vía draw_pixmap con
/// máscara redondeada.
fn draw_thumbnail_flat(
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

/// Imagen en perspectiva: se re-muestrea columna por columna. Cada
/// columna de pantalla se invierte al plano de la card (proyectiva 1D
/// exacta), se muestrea el thumb con bilineal y se recorta con la
/// cobertura analítica del rounded-rect local (con anti-alias
/// anisotrópico según las escalas de la columna).
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

    let (tl_h, tl_v) = if corners.top_left { (rh_l, rv_l) } else { (0.0, 0.0) };
    let (tr_h, tr_v) = if corners.top_right { (rh_r, rv_r) } else { (0.0, 0.0) };
    let (br_h, br_v) = if corners.bottom_right { (rh_r, rv_r) } else { (0.0, 0.0) };
    let (bl_h, bl_v) = if corners.bottom_left { (rh_l, rv_l) } else { (0.0, 0.0) };

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
