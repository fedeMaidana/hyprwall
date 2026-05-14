use super::*;
use crate::wallpaper::{Thumbnail, Wallpaper};

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

fn make_model(n: usize) -> Model {
    let wallpapers = (0..n).map(|i| fake_wallpaper(&format!("w{i}"))).collect();
    let picker = Picker::new(wallpapers, 0);
    Model::new(picker, 1280, 560)
}

fn has_redraw(cmds: &[Cmd]) -> bool {
    cmds.iter().any(|c| matches!(c, Cmd::Redraw))
}

#[test]
fn select_next_moves_selection_and_requests_redraw() {
    let mut m = make_model(3);
    let cmds = update(&mut m, Msg::SelectNext);
    assert_eq!(m.picker.selected(), 1);
    assert!(has_redraw(&cmds));
}

#[test]
fn arrow_with_single_wallpaper_is_a_noop() {
    let mut m = make_model(1);
    let cmds = update(&mut m, Msg::SelectNext);
    assert_eq!(m.picker.selected(), 0);
    assert!(cmds.is_empty());
}

#[test]
fn full_navigate_and_apply_sequence() {
    let mut m = make_model(3);
    let _ = update(&mut m, Msg::SelectNext);
    let _ = update(&mut m, Msg::SelectNext);
    let cmds = update(&mut m, Msg::Apply);

    assert_eq!(m.picker.selected(), 2);
    match cmds.as_slice() {
        [Cmd::ApplyWallpaper(path)] => assert_eq!(path.file_name().unwrap(), "w2.png"),
        other => panic!("esperaba [Cmd::ApplyWallpaper], got {other:?}"),
    }

    let path = PathBuf::from("/tmp/w2.png");
    let cmds = update(&mut m, Msg::WallpaperApplied(path));
    assert!(matches!(cmds.as_slice(), [Cmd::Exit]));
}

#[test]
fn apply_failure_keeps_us_alive() {
    let mut m = make_model(2);
    let cmds = update(
        &mut m,
        Msg::WallpaperFailed {
            path: PathBuf::from("/tmp/w0.png"),
            error: "swww explotó".into(),
        },
    );
    assert!(cmds.is_empty());
}

#[test]
fn scale_change_before_configure_only_sets_buffer_scale() {
    let mut m = make_model(3);
    let cmds = update(&mut m, Msg::ScaleChanged(2));
    assert_eq!(m.scale, 2);
    assert_eq!(cmds.len(), 1);
    assert!(matches!(cmds[0], Cmd::SetBufferScale(2)));
}

#[test]
fn scale_change_after_configure_also_redraws() {
    let mut m = make_model(3);
    let _ = update(
        &mut m,
        Msg::Configured {
            width: 1920,
            height: 1080,
        },
    );
    let cmds = update(&mut m, Msg::ScaleChanged(2));
    assert!(matches!(cmds[0], Cmd::SetBufferScale(2)));
    assert!(has_redraw(&cmds));
}

#[test]
fn scale_unchanged_is_a_noop() {
    let mut m = make_model(3);
    let _ = update(&mut m, Msg::ScaleChanged(2));
    let cmds = update(&mut m, Msg::ScaleChanged(2));
    assert!(cmds.is_empty());
}

#[test]
fn invalid_scale_is_rejected() {
    let mut m = make_model(3);
    let cmds = update(&mut m, Msg::ScaleChanged(0));
    assert_eq!(m.scale, 1);
    assert!(cmds.is_empty());
}

#[test]
fn configure_redraws_only_when_something_changes() {
    let mut m = make_model(3);

    let cmds = update(
        &mut m,
        Msg::Configured {
            width: 1920,
            height: 1080,
        },
    );
    assert!(has_redraw(&cmds));

    let cmds = update(
        &mut m,
        Msg::Configured {
            width: 1920,
            height: 1080,
        },
    );
    assert!(cmds.is_empty());

    let cmds = update(
        &mut m,
        Msg::Configured {
            width: 2560,
            height: 1440,
        },
    );
    assert!(has_redraw(&cmds));
}

#[test]
fn quit_exits() {
    let mut m = make_model(2);
    let cmds = update(&mut m, Msg::Quit);
    assert!(matches!(cmds.as_slice(), [Cmd::Exit]));
}

#[test]
fn click_via_pointer_pressed_at() {
    let mut m = make_model(5);
    let cmds = update(&mut m, Msg::PointerPressedAt { x: 100.0, y: 100.0 });
    assert!(cmds.is_empty(), "sin layout no hay nada que hittear");

    let _layout = m.picker.recompute_layout(1280, 560);
    let (hit_x, hit_y) = (1280.0 / 2.0, 560.0 / 2.0);
    assert_eq!(m.picker.wallpaper_at(hit_x, hit_y), Some(0));

    let cmds = update(&mut m, Msg::PointerPressedAt { x: hit_x, y: hit_y });
    assert!(matches!(cmds.as_slice(), [Cmd::ApplyWallpaper(_)]));
}

#[test]
fn click_on_different_card_changes_selection_and_redraws() {
    let mut m = make_model(5);
    let _ = m.picker.recompute_layout(1280, 560);

    let mut hit: Option<(f64, f64, usize)> = None;
    for x in (0..1280).step_by(20) {
        for y in (0..560).step_by(20) {
            if let Some(idx) = m.picker.wallpaper_at(x as f64, y as f64)
                && idx != 0
            {
                hit = Some((x as f64, y as f64, idx));
                break;
            }
        }
        if hit.is_some() {
            break;
        }
    }
    let (x, y, idx) = hit.expect("debería haber una card no seleccionada visible");

    let cmds = update(&mut m, Msg::PointerPressedAt { x, y });
    assert_eq!(m.picker.selected(), idx);
    assert!(cmds.iter().any(|c| matches!(c, Cmd::ApplyWallpaper(_))));
    assert!(has_redraw(&cmds));
}
