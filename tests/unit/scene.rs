use super::*;
use crate::layout::CardSlot;
use crate::wallpaper::Thumbnail;
use std::path::PathBuf;

fn sel(selected: usize, hovered: Option<usize>) -> Selection {
    Selection {
        selected,
        hovered,
        applied: None,
    }
}

fn build<'a>(
    layout: &Layout,
    wallpapers: &'a [Wallpaper],
    selected: usize,
    hovered: Option<usize>,
) -> Scene<'a> {
    build_scene(
        1280,
        560,
        layout,
        wallpapers,
        sel(selected, hovered),
        DynamicColors::FALLBACK,
    )
}

fn count_strokes(scene: &Scene<'_>) -> usize {
    scene
        .commands
        .iter()
        .filter(|c| matches!(c, DrawCmd::StrokeRoundRect { .. }))
        .count()
}

fn fake_wallpaper(label: &str) -> Wallpaper {
    Wallpaper {
        path: PathBuf::from(format!("/tmp/{label}.png")),
        label: label.to_owned(),
        thumb: Thumbnail {
            width: 1,
            height: 1,
            rgba: vec![0, 0, 0, 255],
        },
    }
}

fn three_wallpapers() -> Vec<Wallpaper> {
    vec![
        fake_wallpaper("a"),
        fake_wallpaper("b"),
        fake_wallpaper("c"),
    ]
}

fn fake_layout(count: usize) -> Layout {
    let cards: Vec<CardSlot> = (0..count)
        .map(|i| CardSlot {
            index: i,
            rect: Rect {
                x: (i as i32) * 220,
                y: 100,
                w: 200,
                h: 284,
            },
            offset: i as f32,
            opacity: 1.0,
        })
        .collect();

    let content_right = cards
        .last()
        .map(|card| card.rect.x + card.rect.w)
        .unwrap_or(0);

    Layout {
        cards,
        content_left: 0,
        content_right,
    }
}

fn count_round_rects(scene: &Scene<'_>) -> usize {
    scene
        .commands
        .iter()
        .filter(|c| matches!(c, DrawCmd::RoundRect { .. }))
        .count()
}

#[test]
fn scrim_covers_everything_then_panel_follows() {
    let wallpapers = three_wallpapers();
    let layout = fake_layout(3);
    let scene = build(&layout, &wallpapers, 0, None);

    match scene.commands.first() {
        Some(DrawCmd::RoundRect { rect, color, .. }) => {
            assert_eq!((rect.x, rect.y, rect.w, rect.h), (0, 0, 1280, 560));
            assert_eq!(color.a, Color::SCRIM.a);
        }
        other => panic!("expected fullscreen scrim first, got {:?}", other.is_some()),
    }

    match scene.commands.get(1) {
        Some(DrawCmd::RoundRect { color, .. }) => {
            assert_eq!(color.r, Color::PANEL.r);
            assert_eq!(color.g, Color::PANEL.g);
            assert_eq!(color.b, Color::PANEL.b);
        }
        other => panic!("expected panel RoundRect second, got {:?}", other.is_some()),
    }
}

#[test]
fn selected_card_emits_thumbnail_and_label() {
    let wallpapers = three_wallpapers();
    let layout = fake_layout(3);
    let scene = build(&layout, &wallpapers, 1, None);

    let expected_thumb = &wallpapers[1].thumb;
    let has_b_thumb = scene.commands.iter().any(|c| match c {
        DrawCmd::Thumbnail { thumb, .. } => std::ptr::eq(*thumb, expected_thumb),
        _ => false,
    });
    assert!(has_b_thumb, "thumbnail del selected no encontrado");

    let label_count = scene
        .commands
        .iter()
        .filter(|c| matches!(c, DrawCmd::Text { text, .. } if *text == "b"))
        .count();
    assert_eq!(label_count, 2, "esperaba shadow + label, got {label_count}");
}

#[test]
fn hovering_non_selected_adds_ring_and_label() {
    let wallpapers = three_wallpapers();
    let layout = fake_layout(3);
    let without = build(&layout, &wallpapers, 0, None);
    let with_hover = build(&layout, &wallpapers, 0, Some(2));

    assert_eq!(count_strokes(&with_hover), count_strokes(&without) + 1);

    let hovered_labels = with_hover
        .commands
        .iter()
        .filter(|c| matches!(c, DrawCmd::Text { text, .. } if *text == "c"))
        .count();
    assert_eq!(hovered_labels, 2, "esperaba shadow + label del hovereado");
}

#[test]
fn hovering_the_selected_card_is_a_noop() {
    let wallpapers = three_wallpapers();
    let layout = fake_layout(3);
    let without = build(&layout, &wallpapers, 0, None);
    let with_hover = build(&layout, &wallpapers, 0, Some(0));
    assert_eq!(with_hover.commands.len(), without.commands.len());
}

#[test]
fn applied_badge_marks_the_current_wallpaper() {
    let wallpapers = three_wallpapers();
    let layout = fake_layout(3);

    let selection = Selection {
        selected: 0,
        hovered: None,
        applied: Some(1),
    };
    let with_badge = build_scene(
        1280,
        560,
        &layout,
        &wallpapers,
        selection,
        DynamicColors::FALLBACK,
    );
    let without_badge = build(&layout, &wallpapers, 0, None);

    assert_eq!(
        count_round_rects(&with_badge),
        count_round_rects(&without_badge) + 2,
        "el badge agrega dos círculos (fondo + acento)"
    );
}

#[test]
fn empty_layout_emits_scrim_panel_and_hints() {
    let wallpapers = three_wallpapers();
    let layout = Layout::empty();
    let scene = build(&layout, &wallpapers, 0, None);
    assert_eq!(scene.commands.len(), 3);
    assert!(matches!(scene.commands[0], DrawCmd::RoundRect { .. }));
    assert!(matches!(scene.commands[1], DrawCmd::RoundRect { .. }));
    assert!(matches!(scene.commands[2], DrawCmd::Text { .. }));
}
