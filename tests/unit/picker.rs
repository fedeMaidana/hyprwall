use super::*;
use crate::wallpaper::Thumbnail;

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

fn make_picker(n: usize) -> Picker {
    let wallpapers = (0..n).map(|i| fake_wallpaper(&format!("w{i}"))).collect();
    Picker::new(wallpapers, 0)
}

#[test]
fn select_next_wraps() {
    let mut p = make_picker(3);
    assert_eq!(p.selected(), 0);
    assert!(p.select_next());
    assert_eq!(p.selected(), 1);
    assert!(p.select_next());
    assert_eq!(p.selected(), 2);
    assert!(p.select_next());
    assert_eq!(p.selected(), 0);
}

#[test]
fn select_prev_wraps() {
    let mut p = make_picker(3);
    assert!(p.select_prev());
    assert_eq!(p.selected(), 2);
    assert!(p.select_prev());
    assert_eq!(p.selected(), 1);
}

#[test]
fn select_is_noop_with_single_wallpaper() {
    let mut p = make_picker(1);
    assert!(!p.select_next());
    assert!(!p.select_prev());
    assert_eq!(p.selected(), 0);
}

#[test]
fn initial_selected_is_clamped() {
    let wallpapers = vec![fake_wallpaper("a"), fake_wallpaper("b")];
    let p = Picker::new(wallpapers, 99);
    assert_eq!(p.selected(), 1);
}

#[test]
fn select_index_returns_false_if_unchanged() {
    let mut p = make_picker(3);
    assert!(!p.select_index(0));
    assert!(p.select_index(2));
    assert!(!p.select_index(2));
    assert_eq!(p.selected(), 2);
}

#[test]
fn clear_hover_only_signals_when_was_set() {
    let mut p = make_picker(2);
    assert!(!p.clear_hover());

    p.hovered = Some(0);
    assert!(p.clear_hover());
    assert_eq!(p.hovered(), None);
}
