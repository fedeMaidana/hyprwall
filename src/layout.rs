use crate::{geometry::Rect, style};

/// Una card ya posicionada por el layout.
#[derive(Clone, Copy, Debug)]
pub struct CardSlot {
    pub index: usize,
    pub rect: Rect,
    /// Offset fraccional (en cards) respecto del centro del carrusel,
    /// CON signo: negativo = a la izquierda. `offset.abs()` es la
    /// distancia; el signo orienta el tilt 3D.
    pub offset: f32,
    /// 1.0 = sólida; < 1.0 mientras se desvanece contra el borde del panel.
    pub opacity: f32,
}

#[derive(Clone)]
pub struct Layout {
    pub cards: Vec<CardSlot>,
    /// Borde izquierdo del contenido EN REPOSO. El panel se dimensiona con
    /// esto (y no con las cards visibles) para no "respirar" durante la
    /// animación de scroll.
    pub content_left: i32,
    /// Borde derecho del contenido en reposo.
    pub content_right: i32,
}

impl Layout {
    pub fn empty() -> Self {
        Self {
            cards: Vec::new(),
            content_left: 0,
            content_right: 0,
        }
    }
}

/// Layout continuo del carrusel. `pos` vive en "espacio de índices":
/// 2.5 significa a mitad de camino entre la card 2 y la 3. Con `pos`
/// entera el resultado es idéntico al layout discreto original; con `pos`
/// fraccional cada card interpola entre slots vecinos, lo que produce el
/// desplazamiento fluido.
pub fn compute(width: u32, height: u32, wallpaper_count: usize, pos: f32) -> Layout {
    if wallpaper_count == 0 {
        return Layout::empty();
    }

    let width = width as i32;
    let height = height as i32;
    let count = wallpaper_count as f32;

    let slots = build_slot_table(width, height, wallpaper_count);

    let content_min_x = style::panel::MIN_SCREEN_MARGIN + style::panel::HORIZONTAL_PADDING;
    let content_max_x = width - style::panel::MIN_SCREEN_MARGIN - style::panel::HORIZONTAL_PADDING;

    let mut placed: Vec<(f32, CardSlot)> = Vec::new();

    for index in 0..wallpaper_count {
        // Offset con wrap al rango [-count/2, count/2).
        let mut u = (index as f32 - pos).rem_euclid(count);
        if u >= count / 2.0 {
            u -= count;
        }

        let Some(rect) = interpolated_slot(&slots, u) else {
            continue;
        };

        let opacity = edge_opacity(rect, content_min_x, content_max_x);
        if opacity <= 0.0 {
            continue;
        }

        placed.push((
            u,
            CardSlot {
                index,
                rect,
                offset: u,
                opacity,
            },
        ));
    }

    placed.sort_by(|a, b| a.0.total_cmp(&b.0));

    Layout {
        cards: placed.into_iter().map(|(_, card)| card).collect(),
        content_left: slots.rest_left,
        content_right: slots.rest_right,
    }
}

/// Geometría de los slots a distancia entera del centro: `right[k]` /
/// `left[k]` es el slot a k cards del centro (índice 0 = card activa).
/// Incluye EDGE_EXTRA_SLOTS más allá del último que cabe en el panel: son
/// los puntos por los que las cards entran y salen deslizándose.
struct SlotTable {
    right: Vec<Rect>,
    left: Vec<Rect>,
    rest_left: i32,
    rest_right: i32,
}

fn build_slot_table(width: i32, height: i32, wallpaper_count: usize) -> SlotTable {
    let gap = style::card::GAP;
    let extra = style::scroll::EDGE_EXTRA_SLOTS;

    let active_w = style::card::WIDTH;
    let active_h = style::card::HEIGHT;
    let active_x = (width - active_w) / 2;
    let active_group_h = active_h + style::label::SELECTED_STRIP_HEIGHT;
    let active_y = (height - active_group_h) / 2;

    let active = Rect {
        x: active_x,
        y: active_y,
        w: active_w,
        h: active_h,
    };

    let content_min_x = style::panel::MIN_SCREEN_MARGIN + style::panel::HORIZONTAL_PADDING;
    let content_max_x = width - style::panel::MIN_SCREEN_MARGIN - style::panel::HORIZONTAL_PADDING;

    // En reposo (pos entera) los offsets con wrap caen en
    // [-count/2, count/2), así que la izquierda recibe la mitad "grande"
    // cuando el total es par — igual que el layout original.
    let max_left = wallpaper_count / 2;
    let max_right = wallpaper_count.saturating_sub(1) / 2;

    let mut right = vec![active];
    let mut left = vec![active];

    let mut right_edge = active_x + active_w;
    let mut left_edge = active_x;

    // Último k por lado cuyo slot cabe entero dentro del panel.
    let mut fit_right = 0usize;
    let mut fit_left = 0usize;

    let mut k = 1usize;
    loop {
        let (w, h) = inactive_card_size(k);
        let group_h = h + style::label::SELECTED_STRIP_HEIGHT;
        let y = (height - group_h) / 2;

        let want_right = k <= (fit_right + extra).min(max_right + extra);
        let want_left = k <= (fit_left + extra).min(max_left + extra);

        if !want_right && !want_left {
            break;
        }

        if want_right {
            let x = right_edge + gap;
            right.push(Rect { x, y, w, h });
            right_edge = x + w;
            if fit_right == k - 1 && x + w <= content_max_x {
                fit_right = k;
            }
        }

        if want_left {
            let x = left_edge - gap - w;
            left.push(Rect { x, y, w, h });
            left_edge = x;
            if fit_left == k - 1 && x >= content_min_x {
                fit_left = k;
            }
        }

        k += 1;
        if k > 128 {
            break;
        }
    }

    let used_right = fit_right.min(max_right);
    let used_left = fit_left.min(max_left);

    let rest_right = {
        let rect = right[used_right];
        rect.x + rect.w
    };
    let rest_left = left[used_left].x;

    SlotTable {
        right,
        left,
        rest_left,
        rest_right,
    }
}

/// Rect de una card a distancia fraccional `u`: interpola linealmente
/// entre los slots enteros vecinos. Con `u` entero devuelve el slot
/// exacto, así el reposo queda pixel-perfect.
fn interpolated_slot(slots: &SlotTable, u: f32) -> Option<Rect> {
    let lo = u.floor();
    let t = u - lo;

    let a = integer_slot(slots, lo)?;
    if t <= f32::EPSILON {
        return Some(a);
    }
    let b = integer_slot(slots, lo + 1.0)?;

    Some(Rect {
        x: lerp_i32(a.x, b.x, t),
        y: lerp_i32(a.y, b.y, t),
        w: lerp_i32(a.w, b.w, t),
        h: lerp_i32(a.h, b.h, t),
    })
}

fn integer_slot(slots: &SlotTable, k: f32) -> Option<Rect> {
    let k = k as i64;
    if k >= 0 {
        slots.right.get(k as usize).copied()
    } else {
        slots.left.get(k.unsigned_abs() as usize).copied()
    }
}

fn lerp_i32(a: i32, b: i32, t: f32) -> i32 {
    (a as f32 + (b as f32 - a as f32) * t).round() as i32
}

/// Opacidad según cuánto invade la card el padding del panel: a
/// FADE_RANGE px de overhang ya es invisible. Como FADE_RANGE es menor
/// que el padding, ninguna card llega a asomarse fuera del panel.
fn edge_opacity(rect: Rect, content_min_x: i32, content_max_x: i32) -> f32 {
    let overhang = (content_min_x - rect.x)
        .max(rect.x + rect.w - content_max_x)
        .max(0);

    1.0 - overhang as f32 / style::scroll::FADE_RANGE as f32
}

fn inactive_card_size(distance: usize) -> (i32, i32) {
    let distance = distance.max(1) as i32;

    let scale_percent = (100 - (distance - 1) * style::card::INACTIVE_SCALE_STEP_PERCENT)
        .max(style::card::INACTIVE_MIN_SCALE_PERCENT);

    let width = style::card::INACTIVE_WIDTH * scale_percent / 100;
    let height = style::card::INACTIVE_HEIGHT * scale_percent / 100;

    (width.max(1), height.max(1))
}
