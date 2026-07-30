use crate::{
    geometry::{Corners, Rect},
    layout::Layout,
    style::{self, Color},
    wallpaper::{Thumbnail, Wallpaper},
};

#[derive(Clone)]
pub enum DrawCmd<'a> {
    RoundRect {
        rect: Rect,
        radius: i32,
        corners: Corners,
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
    selected_index: usize,
    hovered_index: Option<usize>,
    panel_color: Color,
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

    scene.push(DrawCmd::RoundRect {
        rect: carousel_panel_rect(app_width, app_height, layout),
        radius: style::panel::RADIUS,
        corners: Corners::ALL,
        color: panel_color,
    });

    for (idx, image_rect) in &layout.cards {
        let selected = *idx == selected_index;
        let hovered = hovered_index == Some(*idx);
        let wallpaper = &wallpapers[*idx];
        let visible_card_rect = card_rect_with_label_area(*image_rect);

        if hovered && !selected {
            scene.push(DrawCmd::RoundRect {
                rect: outset_rect(visible_card_rect, 4),
                radius: style::card::RADIUS + 4,
                corners: Corners::ALL,
                color: Color::HOVER,
            });
        }

        if selected {
            push_selected_card(&mut scene, *image_rect, wallpaper);
        } else {
            let distance = wallpaper_distance(*idx, selected_index, wallpapers.len());
            push_inactive_card(&mut scene, visible_card_rect, wallpaper, distance);
        }
    }

    scene
}

fn push_selected_card<'a>(scene: &mut Scene<'a>, image_rect: Rect, wallpaper: &'a Wallpaper) {
    let card_rect = card_rect_with_label_area(image_rect);
    scene.push(DrawCmd::RoundRect {
        rect: outset_rect(card_rect, 10),
        radius: style::card::RADIUS + 10,
        corners: Corners::ALL,
        color: Color::ACTIVE_GLOW,
    });
    scene.push(DrawCmd::RoundRect {
        rect: outset_rect(card_rect, 1),
        radius: style::card::RADIUS + 1,
        corners: Corners::ALL,
        color: Color::ACTIVE_BORDER,
    });
    scene.push(DrawCmd::Thumbnail {
        rect: card_rect,
        radius: style::card::RADIUS,
        corners: Corners::ALL,
        thumb: &wallpaper.thumb,
    });
    let overlay_rect = Rect {
        x: image_rect.x,
        y: image_rect.y + image_rect.h - style::label::GRADIENT_LIFT,
        w: image_rect.w,
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
        x: image_rect.x - 12,
        y: image_rect.y + image_rect.h + style::label::TOP_GAP,
        w: image_rect.w + 24,
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
