use super::*;
use crate::wallpaper::Thumbnail;
use std::time::Duration;

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

#[test]
fn navigation_moves_on_the_first_frame_after_input() {
    let actions: [fn(&mut Picker) -> bool; 4] = [
        Picker::select_next,
        Picker::select_prev,
        |p| p.scroll_by(1.0),
        |p| p.select_index(1),
    ];

    for action in actions {
        let mut p = make_picker(5);
        assert!(action(&mut p));
        let input_time = p.last_tick.expect("input must start the animation clock");

        assert!(p.tick_at(input_time + Duration::from_millis(16)));
        assert!(p.pos.abs() > 0.0);
        assert!(p.pos.abs() < 1.0);
    }
}

#[test]
fn animation_progress_is_independent_of_frame_rate() {
    let start = Instant::now();
    let mut slow = make_picker(5);
    let mut fast = make_picker(5);
    slow.retarget_by_at(1.0, start);
    fast.retarget_by_at(1.0, start);

    for frame in 1..=3 {
        slow.tick_at(start + Duration::from_millis(frame * 100));
    }
    for frame in 1..=18 {
        fast.tick_at(start + Duration::from_secs_f64(frame as f64 / 60.0));
    }

    assert!((slow.pos - fast.pos).abs() < 0.00001);
    assert!((slow.vel - fast.vel).abs() < 0.00002);
}

#[test]
fn retarget_advances_to_input_time_using_the_old_target() {
    let start = Instant::now();
    let input_time = start + Duration::from_millis(100);
    let mut p = make_picker(5);
    let mut reference = make_picker(5);
    p.retarget_by_at(1.0, start);
    reference.retarget_by_at(1.0, start);
    reference.tick_at(input_time);

    p.retarget_by_at(-2.0, input_time);

    assert_eq!(p.pos, reference.pos);
    assert_eq!(p.vel, reference.vel);
    assert!(p.vel > 0.0, "retarget must preserve the current momentum");
    assert_eq!(p.target, -1.0);
    assert_eq!(p.last_tick, Some(input_time));
}

#[test]
fn retarget_preserves_the_step_when_the_old_target_wraps_on_settling() {
    let start = Instant::now();
    let mut p = make_picker(3);
    p.retarget_by_at(4.0, start);

    let next_input = start + Duration::from_secs(2);
    p.retarget_by_at(1.0, next_input);

    assert_eq!(p.pos, 1.0);
    assert_eq!(p.target, 2.0);
    assert_eq!(p.vel, 0.0);
    assert!(p.tick_at(next_input + Duration::from_millis(16)));
    assert!(p.pos > 1.0 && p.pos < 2.0);
}

#[test]
fn a_long_pause_settles_instead_of_running_in_slow_motion() {
    let start = Instant::now();
    let mut p = make_picker(3);
    p.retarget_by_at(-1.0, start);

    assert!(!p.tick_at(start + Duration::from_secs(5)));
    assert_eq!(p.pos, 2.0);
    assert_eq!(p.target, 2.0);
    assert_eq!(p.vel, 0.0);
    assert_eq!(p.last_tick, None);

    let next_input = start + Duration::from_secs(10);
    p.retarget_by_at(-1.0, next_input);
    assert!(p.tick_at(next_input + Duration::from_millis(16)));
    assert!(p.pos > 1.0 && p.pos < 2.0);
}

#[test]
fn a_single_step_settles_within_half_a_second() {
    let start = Instant::now();
    let mut p = make_picker(5);
    p.retarget_by_at(1.0, start);

    assert!(!p.tick_at(start + Duration::from_millis(500)));
    assert_eq!(p.pos, 1.0);
    assert_eq!(p.vel, 0.0);
}
