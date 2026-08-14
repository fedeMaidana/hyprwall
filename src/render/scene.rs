use crate::{
    geometry::{Corners, Rect},
    hyprcolor::DynamicColors,
    layout::Layout,
    style::{self, Color},
    wallpaper::{Thumbnail, Wallpaper},
};

/// Which cards are highlighted: keyboard selection, pointer hover and the
/// wallpaper currently applied on the desktop.
#[derive(Clone, Copy, Debug)]
pub struct Selection {
    pub selected: usize,
    pub hovered: Option<usize>,
    pub applied: Option<usize>,
}

#[derive(Clone)]
pub enum DrawCmd<'a> {
    RoundRect {
        rect: Rect,
        radius: i32,
        corners: Corners,
        color: Color,
    },
    StrokeRoundRect {
        rect: Rect,
        radius: i32,
        corners: Corners,
        width: f32,
        color: Color,
    },
    VerticalGradient {
        rect: Rect,
        radius: i32,
        corners: Corners,
        bottom_color: Color,
        top_alpha: u8,
    },
    Thumbnail {
        rect: Rect,
        radius: i32,
        corners: Corners,
        thumb: &'a Thumbnail,
        /// Paneo horizontal (parallax) de la imagen dentro de la card,
        /// en px lógicos. Positivo = imagen corrida a la derecha.
        shift_x: i32,
        /// Opacidad de la card (1.0 = sólida); anima el desvanecimiento
        /// contra los bordes del panel durante el scroll.
        opacity: f32,
    },
    Text {
        rect: Rect,
        text: &'a str,
        font_size: f32,
        color: Color,
    },
    /// Abre un grupo con rotación 3D en perspectiva real: los comandos
    /// siguientes se proyectan como un plano rotado `tilt` (fracción del
    /// ángulo máximo, con signo) alrededor del eje vertical del pivote,
    /// hasta el próximo [`DrawCmd::EndTilt`]. El texto no se transforma
    /// (las etiquetas quedan derechas y legibles).
    BeginTilt {
        pivot_x: f32,
        pivot_y: f32,
        /// Fracción del ángulo máximo, en [-1, 1]. Positivo = card a la
        /// derecha del centro (mira hacia adentro).
        tilt: f32,
        /// Ancho de referencia de la card (px): fija la distancia de
        /// cámara en unidades de card.
        ref_width: f32,
    },
    EndTilt,
}

pub struct Scene<'a> {
    pub commands: Vec<DrawCmd<'a>>,
}

impl<'a> Scene<'a> {
    fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }
    fn push(&mut self, cmd: DrawCmd<'a>) {
        self.commands.push(cmd);
    }
}

pub fn build_scene<'a>(
    app_width: u32,
    app_height: u32,
    layout: &Layout,
    wallpapers: &'a [Wallpaper],
    selection: Selection,
    colors: DynamicColors,
) -> Scene<'a> {
    let mut scene = Scene::new();

    // Fullscreen scrim first: gives every pixel alpha so the compositor
    // blur reaches the entire screen, not just the panel.
    scene.push(DrawCmd::RoundRect {
        rect: Rect {
            x: 0,
            y: 0,
            w: app_width as i32,
            h: app_height as i32,
        },
        radius: 0,
        corners: Corners::ALL,
        color: Color::SCRIM,
    });

    let panel = carousel_panel_rect(app_width, app_height, layout);

    scene.push(DrawCmd::RoundRect {
        rect: panel,
        radius: style::panel::RADIUS,
        corners: Corners::ALL,
        color: colors.panel,
    });

    for card in &layout.cards {
        let idx = card.index;
        let opacity = card.opacity;
        let selected = idx == selection.selected;
        let hovered = selection.hovered == Some(idx);
        let wallpaper = &wallpapers[idx];
        let visible_card_rect = card_rect_with_label_area(card.rect);

        // Rotación 3D de las cards laterales, continua durante el scroll
        // (crece con el offset al centro).
        let tilt = card_tilt(card.offset);
        let tilted = tilt.abs() > 0.005;
        if tilted {
            scene.push(DrawCmd::BeginTilt {
                pivot_x: visible_card_rect.x as f32 + visible_card_rect.w as f32 / 2.0,
                pivot_y: visible_card_rect.y as f32 + visible_card_rect.h as f32 / 2.0,
                tilt,
                ref_width: visible_card_rect.w as f32,
            });
        }

        if hovered && !selected {
            scene.push(DrawCmd::StrokeRoundRect {
                rect: outset_rect(visible_card_rect, style::selection::HOVER_OUTSET),
                radius: style::card::RADIUS + style::selection::HOVER_OUTSET,
                corners: Corners::ALL,
                width: style::selection::HOVER_WIDTH,
                color: fade(
                    colors.accent.with_alpha(style::selection::HOVER_ALPHA),
                    opacity,
                ),
            });
        }

        let shift_x = parallax_shift(card.rect, app_width as i32);

        if selected {
            push_selected_card(
                &mut scene,
                card.rect,
                wallpaper,
                colors.accent,
                shift_x,
                opacity,
            );
        } else {
            push_inactive_card(
                &mut scene,
                visible_card_rect,
                wallpaper,
                card.offset.abs(),
                shift_x,
                opacity,
            );

            if hovered {
                push_card_label(&mut scene, visible_card_rect, wallpaper, opacity);
            }
        }

        if selection.applied == Some(idx) {
            push_applied_badge(&mut scene, visible_card_rect, colors.accent, opacity);
        }

        if tilted {
            scene.push(DrawCmd::EndTilt);
        }
    }

    push_keyboard_hints(&mut scene, panel);

    scene
}

fn push_selected_card<'a>(
    scene: &mut Scene<'a>,
    image_rect: Rect,
    wallpaper: &'a Wallpaper,
    accent: Color,
    shift_x: i32,
    opacity: f32,
) {
    let card_rect = card_rect_with_label_area(image_rect);
    scene.push(DrawCmd::RoundRect {
        rect: outset_rect(card_rect, style::selection::GLOW_OUTSET),
        radius: style::card::RADIUS + style::selection::GLOW_OUTSET,
        corners: Corners::ALL,
        color: fade(accent.with_alpha(style::selection::GLOW_ALPHA), opacity),
    });
    scene.push(DrawCmd::Thumbnail {
        rect: card_rect,
        radius: style::card::RADIUS,
        corners: Corners::ALL,
        thumb: &wallpaper.thumb,
        shift_x,
        opacity,
    });
    scene.push(DrawCmd::StrokeRoundRect {
        rect: outset_rect(card_rect, style::selection::RING_OUTSET),
        radius: style::card::RADIUS + style::selection::RING_OUTSET,
        corners: Corners::ALL,
        width: style::selection::RING_WIDTH,
        color: fade(accent.with_alpha(style::selection::RING_ALPHA), opacity),
    });
    push_card_label(scene, card_rect, wallpaper, opacity);
}

/// Bottom gradient plus the wallpaper name, over any card rect.
fn push_card_label<'a>(
    scene: &mut Scene<'a>,
    card_rect: Rect,
    wallpaper: &'a Wallpaper,
    opacity: f32,
) {
    let image_bottom = card_rect.y + card_rect.h - style::label::SELECTED_STRIP_HEIGHT;

    let overlay_rect = Rect {
        x: card_rect.x,
        y: image_bottom - style::label::GRADIENT_LIFT,
        w: card_rect.w,
        h: style::label::SELECTED_STRIP_HEIGHT + style::label::GRADIENT_LIFT,
    };
    scene.push(DrawCmd::VerticalGradient {
        rect: overlay_rect,
        radius: style::card::RADIUS,
        corners: Corners {
            top_left: false,
            top_right: false,
            bottom_right: true,
            bottom_left: true,
        },
        bottom_color: fade(
            Color {
                r: 0,
                g: 0,
                b: 0,
                a: style::label::GRADIENT_BOTTOM_ALPHA,
            },
            opacity,
        ),
        top_alpha: 0,
    });
    let label_rect = Rect {
        x: card_rect.x - 12,
        y: image_bottom + style::label::TOP_GAP,
        w: card_rect.w + 24,
        h: style::label::SELECTED_STRIP_HEIGHT - style::label::TOP_GAP,
    };
    let shadow_rect = Rect {
        x: label_rect.x + style::label::TEXT_SHADOW_OFFSET_X,
        y: label_rect.y + style::label::TEXT_SHADOW_OFFSET_Y,
        ..label_rect
    };
    scene.push(DrawCmd::Text {
        rect: shadow_rect,
        text: &wallpaper.label,
        font_size: style::label::FONT_SIZE,
        color: fade(Color::TEXT_SHADOW, opacity),
    });
    scene.push(DrawCmd::Text {
        rect: label_rect,
        text: &wallpaper.label,
        font_size: style::label::FONT_SIZE,
        color: fade(Color::TEXT_ON_SELECTED, opacity),
    });
}

fn push_inactive_card<'a>(
    scene: &mut Scene<'a>,
    card_rect: Rect,
    wallpaper: &'a Wallpaper,
    distance: f32,
    shift_x: i32,
    opacity: f32,
) {
    scene.push(DrawCmd::RoundRect {
        rect: card_rect,
        radius: style::card::RADIUS,
        corners: Corners::ALL,
        color: fade(Color::CARD_BG, opacity),
    });
    scene.push(DrawCmd::Thumbnail {
        rect: card_rect,
        radius: style::card::RADIUS,
        corners: Corners::ALL,
        thumb: &wallpaper.thumb,
        shift_x,
        opacity,
    });
    scene.push(DrawCmd::RoundRect {
        rect: card_rect,
        radius: style::card::RADIUS,
        corners: Corners::ALL,
        color: fade(inactive_dim_color(distance), opacity),
    });
    // Glass hairline: separates the card from the panel and dark thumbs.
    scene.push(DrawCmd::StrokeRoundRect {
        rect: card_rect,
        radius: style::card::RADIUS,
        corners: Corners::ALL,
        width: 1.0,
        color: fade(Color::CARD_HAIRLINE, opacity),
    });
}

/// Accent dot marking the wallpaper that is currently applied.
fn push_applied_badge(scene: &mut Scene<'_>, card_rect: Rect, accent: Color, opacity: f32) {
    let outer = style::selection::BADGE_OUTER;
    let inner = style::selection::BADGE_INNER;
    let margin = style::selection::BADGE_MARGIN;

    let outer_rect = Rect {
        x: card_rect.x + card_rect.w - outer - margin,
        y: card_rect.y + margin,
        w: outer,
        h: outer,
    };
    let inset = (outer - inner) / 2;
    let inner_rect = Rect {
        x: outer_rect.x + inset,
        y: outer_rect.y + inset,
        w: inner,
        h: inner,
    };

    scene.push(DrawCmd::RoundRect {
        rect: outer_rect,
        radius: outer / 2,
        corners: Corners::ALL,
        color: fade(Color::CARD_BG, opacity),
    });
    scene.push(DrawCmd::RoundRect {
        rect: inner_rect,
        radius: inner / 2,
        corners: Corners::ALL,
        color: fade(accent, opacity),
    });
}

/// Muted key guide, floating just below the panel.
fn push_keyboard_hints(scene: &mut Scene<'_>, panel: Rect) {
    let hint_rect = Rect {
        x: panel.x,
        y: panel.y + panel.h + style::hints::PANEL_GAP,
        w: panel.w,
        h: style::hints::STRIP_HEIGHT,
    };

    scene.push(DrawCmd::Text {
        rect: hint_rect,
        text: style::hints::TEXT,
        font_size: style::hints::FONT_SIZE,
        color: Color::HINT_TEXT,
    });
}

fn card_rect_with_label_area(image_rect: Rect) -> Rect {
    Rect {
        x: image_rect.x,
        y: image_rect.y,
        w: image_rect.w,
        h: image_rect.h + style::label::SELECTED_STRIP_HEIGHT,
    }
}
fn outset_rect(rect: Rect, amount: i32) -> Rect {
    Rect {
        x: rect.x - amount,
        y: rect.y - amount,
        w: rect.w + amount * 2,
        h: rect.h + amount * 2,
    }
}
/// Fracción de rotación 3D de una card según su offset con signo al
/// centro. Rampa continua: 0 en el centro, ±1 (ángulo máximo) a
/// RAMP_CARDS de distancia. El signo hace que ambos lados "miren" hacia
/// el centro del carrusel.
fn card_tilt(offset: f32) -> f32 {
    (offset / style::card3d::RAMP_CARDS).clamp(-1.0, 1.0)
}

/// Paneo parallax: la imagen se corre en sentido opuesto al offset de la
/// card respecto del centro de pantalla, como una ventana hacia una capa
/// más profunda. Al navegar, las cards cambian de posición y la imagen
/// interior panea con ellas. `draw_thumbnail` recorta el paneo al bleed
/// disponible, así que acá no hace falta clamp.
fn parallax_shift(card_rect: Rect, screen_w: i32) -> i32 {
    let card_center = card_rect.x + card_rect.w / 2;
    let screen_center = screen_w / 2;
    -((card_center - screen_center) * style::parallax::STRENGTH_PERCENT / 100)
}

/// Atenuación continua: crece con la distancia fraccional al centro, así
/// el oscurecimiento anima suave durante el scroll. En reposo (distancias
/// enteras) coincide con los valores del esquema original.
fn inactive_dim_color(distance: f32) -> Color {
    let steps = (distance - 1.0).max(0.0);
    let alpha = style::card::INACTIVE_DIM_BASE_ALPHA as f32
        + steps * style::card::INACTIVE_DIM_STEP_ALPHA as f32;
    Color {
        a: alpha.min(style::card::INACTIVE_DIM_MAX_ALPHA as f32).round() as u8,
        ..Color::CARD_DIM
    }
}

/// Multiplica el alpha de un color por la opacidad de la card.
fn fade(color: Color, opacity: f32) -> Color {
    if opacity >= 1.0 {
        return color;
    }
    color.with_alpha((color.a as f32 * opacity.max(0.0)).round() as u8)
}

/// El panel se dimensiona con los límites del contenido EN REPOSO que
/// reporta el layout (no con las cards visibles): durante el scroll hay
/// cards deslizándose por el padding y el panel no debe "respirar".
fn carousel_panel_rect(app_width: u32, app_height: u32, layout: &Layout) -> Rect {
    let screen_w = app_width as i32;
    let content_w = (layout.content_right - layout.content_left).max(style::card::WIDTH);
    let max_panel_w = screen_w - style::panel::MIN_SCREEN_MARGIN * 2;
    let panel_w = (content_w + style::panel::HORIZONTAL_PADDING * 2)
        .min(max_panel_w)
        .max(style::card::WIDTH + style::panel::HORIZONTAL_PADDING * 2);
    Rect {
        x: (screen_w - panel_w) / 2,
        y: (app_height as i32 - style::panel::HEIGHT) / 2,
        w: panel_w,
        h: style::panel::HEIGHT,
    }
}

#[cfg(test)]
#[path = "../../tests/unit/scene.rs"]
mod tests;
