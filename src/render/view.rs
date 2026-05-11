use fontdue::Font;

use crate::{
    geometry::{Corners, Rect},
    layout::Layout,
    render::{
        primitives::{
            draw_thumbnail, fill_round_rect_corners, fill_vertical_gradient_round_rect_corners,
        },
        text::draw_text_centered_in_rect,
    },
    style::{self, Color},
    wallpaper::Wallpaper,
};

pub fn draw_background(
    canvas: &mut [u8],
    width: u32,
    height: u32,
    app_width: u32,
    app_height: u32,
    layout: &Layout,
    wallpapers: &[Wallpaper],
    selected_index: usize,
    hovered_index: Option<usize>,
    font: &Font,
) {
    let panel = carousel_panel_rect(app_width, app_height, layout);

    fill_round_rect_corners(
        canvas,
        width,
        height,
        panel,
        style::panel::RADIUS,
        Corners::ALL,
        Color::PANEL,
    );

    for (idx, image_rect) in &layout.cards {
        let selected = *idx == selected_index;
        let hovered = hovered_index == Some(*idx);
        let wallpaper = &wallpapers[*idx];

        let visible_card_rect = card_rect_with_label_area(*image_rect);

        if hovered && !selected {
            fill_round_rect_corners(
                canvas,
                width,
                height,
                outset_rect(visible_card_rect, 4),
                style::card::RADIUS + 4,
                Corners::ALL,
                Color::HOVER,
            );
        }

        if selected {
            draw_selected_card(canvas, width, height, *image_rect, wallpaper, font);
        } else {
            let distance = wallpaper_distance(*idx, selected_index, wallpapers.len());

            draw_inactive_card(
                canvas,
                width,
                height,
                visible_card_rect,
                wallpaper,
                distance,
            );
        }
    }
}

fn draw_selected_card(
    canvas: &mut [u8],
    width: u32,
    height: u32,
    image_rect: Rect,
    wallpaper: &Wallpaper,
    font: &Font,
) {
    let card_rect = card_rect_with_label_area(image_rect);

    fill_round_rect_corners(
        canvas,
        width,
        height,
        outset_rect(card_rect, 10),
        style::card::RADIUS + 10,
        Corners::ALL,
        Color::ACTIVE_GLOW,
    );

    fill_round_rect_corners(
        canvas,
        width,
        height,
        outset_rect(card_rect, 1),
        style::card::RADIUS + 1,
        Corners::ALL,
        Color::ACTIVE_BORDER,
    );

    draw_thumbnail(
        canvas,
        width,
        height,
        card_rect,
        style::card::RADIUS,
        Corners::ALL,
        &wallpaper.thumb,
    );

    let overlay_rect = Rect {
        x: image_rect.x,
        y: image_rect.y + image_rect.h - style::label::GRADIENT_LIFT,
        w: image_rect.w,
        h: style::label::SELECTED_STRIP_HEIGHT + style::label::GRADIENT_LIFT,
    };

    fill_vertical_gradient_round_rect_corners(
        canvas,
        width,
        height,
        overlay_rect,
        style::card::RADIUS,
        Corners {
            top_left: false,
            top_right: false,
            bottom_right: true,
            bottom_left: true,
        },
        Color {
            r: 0,
            g: 0,
            b: 0,
            a: style::label::GRADIENT_BOTTOM_ALPHA,
        },
        0,
    );

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

    draw_text_centered_in_rect(
        canvas,
        width,
        height,
        font,
        &wallpaper.label,
        style::label::FONT_SIZE,
        shadow_rect,
        Color::TEXT_SHADOW,
    );

    draw_text_centered_in_rect(
        canvas,
        width,
        height,
        font,
        &wallpaper.label,
        style::label::FONT_SIZE,
        label_rect,
        Color::TEXT_ON_SELECTED,
    );
}

fn draw_inactive_card(
    canvas: &mut [u8],
    width: u32,
    height: u32,
    card_rect: Rect,
    wallpaper: &Wallpaper,
    distance: usize,
) {
    fill_round_rect_corners(
        canvas,
        width,
        height,
        card_rect,
        style::card::RADIUS,
        Corners::ALL,
        Color::CARD_BG,
    );

    draw_thumbnail(
        canvas,
        width,
        height,
        card_rect,
        style::card::RADIUS,
        Corners::ALL,
        &wallpaper.thumb,
    );

    fill_round_rect_corners(
        canvas,
        width,
        height,
        card_rect,
        style::card::RADIUS,
        Corners::ALL,
        inactive_dim_color(distance),
    );
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
                    Some((current_left, current_right)) => {
                        (current_left.min(left), current_right.max(right))
                    }
                    None => (left, right),
                })
            });

    let content_w = content_bounds
        .map(|(left, right)| right - left)
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
