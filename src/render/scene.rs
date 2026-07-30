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
    },
    Text {
        rect: Rect,
        text: &'a str,
        font_size: f32,
        color: Color,
    },
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

    for (idx, image_rect) in &layout.cards {
        let selected = *idx == selection.selected;
        let hovered = selection.hovered == Some(*idx);
        let wallpaper = &wallpapers[*idx];
        let visible_card_rect = card_rect_with_label_area(*image_rect);

        if hovered && !selected {
            scene.push(DrawCmd::StrokeRoundRect {
                rect: outset_rect(visible_card_rect, style::selection::HOVER_OUTSET),
                radius: style::card::RADIUS + style::selection::HOVER_OUTSET,
                corners: Corners::ALL,
                width: style::selection::HOVER_WIDTH,
                color: colors.accent.with_alpha(style::selection::HOVER_ALPHA),
            });
        }

        if selected {
            push_selected_card(&mut scene, *image_rect, wallpaper, colors.accent);
        } else {
            let distance = wallpaper_distance(*idx, selection.selected, wallpapers.len());
            push_inactive_card(&mut scene, visible_card_rect, wallpaper, distance);

            if hovered {
                push_card_label(&mut scene, visible_card_rect, wallpaper);
            }
        }

        if selection.applied == Some(*idx) {
            push_applied_badge(&mut scene, visible_card_rect, colors.accent);
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
) {
    let card_rect = card_rect_with_label_area(image_rect);
    scene.push(DrawCmd::RoundRect {
        rect: outset_rect(card_rect, style::selection::GLOW_OUTSET),
        radius: style::card::RADIUS + style::selection::GLOW_OUTSET,
        corners: Corners::ALL,
        color: accent.with_alpha(style::selection::GLOW_ALPHA),
    });
    scene.push(DrawCmd::Thumbnail {
        rect: card_rect,
        radius: style::card::RADIUS,
        corners: Corners::ALL,
        thumb: &wallpaper.thumb,
    });
    scene.push(DrawCmd::StrokeRoundRect {
        rect: outset_rect(card_rect, style::selection::RING_OUTSET),
        radius: style::card::RADIUS + style::selection::RING_OUTSET,
        corners: Corners::ALL,
        width: style::selection::RING_WIDTH,
        color: accent.with_alpha(style::selection::RING_ALPHA),
    });
    push_card_label(scene, card_rect, wallpaper);
}

/// Bottom gradient plus the wallpaper name, over any card rect.
fn push_card_label<'a>(scene: &mut Scene<'a>, card_rect: Rect, wallpaper: &'a Wallpaper) {
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
        bottom_color: Color {
            r: 0,
            g: 0,
            b: 0,
            a: style::label::GRADIENT_BOTTOM_ALPHA,
        },
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
        color: Color::TEXT_SHADOW,
    });
    scene.push(DrawCmd::Text {
        rect: label_rect,
        text: &wallpaper.label,
        font_size: style::label::FONT_SIZE,
        color: Color::TEXT_ON_SELECTED,
    });
}

fn push_inactive_card<'a>(
    scene: &mut Scene<'a>,
    card_rect: Rect,
    wallpaper: &'a Wallpaper,
    distance: usize,
) {
    scene.push(DrawCmd::RoundRect {
        rect: card_rect,
        radius: style::card::RADIUS,
        corners: Corners::ALL,
        color: Color::CARD_BG,
    });
    scene.push(DrawCmd::Thumbnail {
        rect: card_rect,
        radius: style::card::RADIUS,
        corners: Corners::ALL,
        thumb: &wallpaper.thumb,
    });
    scene.push(DrawCmd::RoundRect {
        rect: card_rect,
        radius: style::card::RADIUS,
        corners: Corners::ALL,
        color: inactive_dim_color(distance),
    });
    // Glass hairline: separates the card from the panel and dark thumbs.
    scene.push(DrawCmd::StrokeRoundRect {
        rect: card_rect,
        radius: style::card::RADIUS,
        corners: Corners::ALL,
        width: 1.0,
        color: Color::CARD_HAIRLINE,
    });
}

/// Accent dot marking the wallpaper that is currently applied.
fn push_applied_badge(scene: &mut Scene<'_>, card_rect: Rect, accent: Color) {
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
        color: Color::CARD_BG,
    });
    scene.push(DrawCmd::RoundRect {
        rect: inner_rect,
        radius: inner / 2,
        corners: Corners::ALL,
        color: accent,
    });
}

/// Muted key guide at the bottom edge of the panel.
fn push_keyboard_hints(scene: &mut Scene<'_>, panel: Rect) {
    let hint_rect = Rect {
        x: panel.x,
        y: panel.y + panel.h - style::hints::STRIP_HEIGHT,
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
fn wallpaper_distance(index: usize, selected: usize, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    let forward = (index + count - selected) % count;
    let backward = (selected + count - index) % count;
    forward.min(backward)
}
fn inactive_dim_color(distance: usize) -> Color {
    let alpha = style::card::INACTIVE_DIM_BASE_ALPHA as usize
        + distance
            .saturating_sub(1)
            .saturating_mul(style::card::INACTIVE_DIM_STEP_ALPHA as usize);
    Color {
        a: alpha.min(style::card::INACTIVE_DIM_MAX_ALPHA as usize) as u8,
        ..Color::CARD_DIM
    }
}
fn carousel_panel_rect(app_width: u32, app_height: u32, layout: &Layout) -> Rect {
    let screen_w = app_width as i32;
    let content_bounds: Option<(i32, i32)> =
        layout
            .cards
            .iter()
            .fold(None, |bounds: Option<(i32, i32)>, (_, rect)| {
                let left = rect.x;
                let right = rect.x + rect.w;
                Some(match bounds {
                    Some((cl, cr)) => (cl.min(left), cr.max(right)),
                    None => (left, right),
                })
            });
    let content_w = content_bounds
        .map(|(l, r)| r - l)
        .unwrap_or(style::card::WIDTH);
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
