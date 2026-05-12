use crate::{geometry::Rect, style};

#[derive(Clone)]
pub struct Layout {
    pub cards: Vec<(usize, Rect)>,
}

impl Layout {
    pub fn empty() -> Self {
        Self { cards: Vec::new() }
    }
}

pub fn compute(width: u32, height: u32, wallpaper_count: usize, selected: usize) -> Layout {
    if wallpaper_count == 0 {
        return Layout::empty();
    }

    let width = width as i32;
    let height = height as i32;

    let selected = selected.min(wallpaper_count - 1);

    let active_w = style::card::WIDTH;
    let active_h = style::card::HEIGHT;
    let gap = style::card::GAP;

    let active_x = (width - active_w) / 2;

    let active_group_h = active_h + style::label::SELECTED_STRIP_HEIGHT;
    let active_y = (height - active_group_h) / 2;

    let active_rect = Rect {
        x: active_x,
        y: active_y,
        w: active_w,
        h: active_h,
    };

    let mut left_cards = Vec::new();
    let mut right_cards = Vec::new();
    let mut placed_indices = vec![selected];

    let mut left_edge = active_x;
    let mut right_edge = active_x + active_w;

    let mut distance = 1;

    while placed_indices.len() < wallpaper_count {
        let (inactive_w, inactive_h) = inactive_card_size(distance);

        let inactive_group_h = inactive_h + style::label::SELECTED_STRIP_HEIGHT;
        let inactive_y = (height - inactive_group_h) / 2;

        let left_x = left_edge - gap - inactive_w;
        let right_x = right_edge + gap;

        let left_fits = left_x >= 0;
        let right_fits = right_x + inactive_w <= width;

        if !left_fits && !right_fits {
            break;
        }

        let left_index = wrapped_index(selected, wallpaper_count, -(distance as isize));
        let right_index = wrapped_index(selected, wallpaper_count, distance as isize);

        if left_fits
            && push_card_once(
                &mut left_cards,
                &mut placed_indices,
                left_index,
                Rect {
                    x: left_x,
                    y: inactive_y,
                    w: inactive_w,
                    h: inactive_h,
                },
            )
        {
            left_edge = left_x;
        }

        if right_fits
            && push_card_once(
                &mut right_cards,
                &mut placed_indices,
                right_index,
                Rect {
                    x: right_x,
                    y: inactive_y,
                    w: inactive_w,
                    h: inactive_h,
                },
            )
        {
            right_edge = right_x + inactive_w;
        }

        distance += 1;
    }

    left_cards.reverse();

    let mut cards = Vec::with_capacity(left_cards.len() + 1 + right_cards.len());
    cards.extend(left_cards);
    cards.push((selected, active_rect));
    cards.extend(right_cards);

    Layout { cards }
}

fn inactive_card_size(distance: usize) -> (i32, i32) {
    let distance = distance.max(1) as i32;

    let scale_percent = (100 - (distance - 1) * style::card::INACTIVE_SCALE_STEP_PERCENT)
        .max(style::card::INACTIVE_MIN_SCALE_PERCENT);

    let width = style::card::INACTIVE_WIDTH * scale_percent / 100;
    let height = style::card::INACTIVE_HEIGHT * scale_percent / 100;

    (width.max(1), height.max(1))
}

fn wrapped_index(selected: usize, count: usize, offset: isize) -> usize {
    let count = count as isize;
    let selected = selected as isize;

    (selected + offset).rem_euclid(count) as usize
}

fn push_card_once(
    cards: &mut Vec<(usize, Rect)>,
    placed_indices: &mut Vec<usize>,
    index: usize,
    rect: Rect,
) -> bool {
    if placed_indices.contains(&index) {
        return false;
    }

    placed_indices.push(index);
    cards.push((index, rect));

    true
}
