use crate::picker::Picker;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum Msg {
    SelectPrev,
    SelectNext,

    HoverAt { x: f64, y: f64 },
    ClearHover,
    PointerPressedAt { x: f64, y: f64 },

    Apply,
    Quit,

    Configured { width: u32, height: u32 },
    ScaleChanged(i32),

    WallpaperApplied(PathBuf),
    WallpaperFailed { path: PathBuf, error: String },
}

#[derive(Debug)]
pub enum Cmd {
    Redraw,
    ApplyWallpaper(PathBuf),
    SetBufferScale(i32),
    Exit,
}

pub struct Model {
    pub picker: Picker,
    pub scale: i32,
    pub logical_width: u32,
    pub logical_height: u32,
    pub configured: bool,
}

impl Model {
    pub fn new(picker: Picker, default_width: u32, default_height: u32) -> Self {
        Self {
            picker,
            scale: 1,
            logical_width: default_width,
            logical_height: default_height,
            configured: false,
        }
    }
}

pub fn update(model: &mut Model, msg: Msg) -> Vec<Cmd> {
    match msg {
        Msg::SelectPrev => redraw_if(model.picker.select_prev()),
        Msg::SelectNext => redraw_if(model.picker.select_next()),

        Msg::HoverAt { x, y } => redraw_if(model.picker.hover_at(x, y)),
        Msg::ClearHover => redraw_if(model.picker.clear_hover()),

        Msg::PointerPressedAt { x, y } => {
            let Some(idx) = model.picker.wallpaper_at(x, y) else {
                return vec![];
            };
            let selection_changed = model.picker.select_index(idx);
            let mut cmds = vec![Cmd::ApplyWallpaper(model.picker.current().path.clone())];
            if selection_changed {
                cmds.push(Cmd::Redraw);
            }
            cmds
        }

        Msg::Apply => vec![Cmd::ApplyWallpaper(model.picker.current().path.clone())],

        Msg::Quit => vec![Cmd::Exit],

        Msg::Configured { width, height } => {
            let size_changed = model.logical_width != width || model.logical_height != height;
            let first_configure = !model.configured;

            model.logical_width = width;
            model.logical_height = height;
            model.configured = true;

            if size_changed || first_configure {
                vec![Cmd::Redraw]
            } else {
                vec![]
            }
        }

        Msg::ScaleChanged(new_scale) => {
            if new_scale < 1 || new_scale == model.scale {
                return vec![];
            }
            log::info!("HiDPI scale changed: {} -> {new_scale}", model.scale);
            model.scale = new_scale;

            let mut cmds = vec![Cmd::SetBufferScale(new_scale)];
            if model.configured {
                cmds.push(Cmd::Redraw);
            }
            cmds
        }

        Msg::WallpaperApplied(path) => {
            log::info!("wallpaper aplicado: {}", path.display());
            vec![Cmd::Exit]
        }

        Msg::WallpaperFailed { path, error } => {
            log::error!("no se pudo aplicar {}: {error}", path.display());
            vec![]
        }
    }
}

fn redraw_if(changed: bool) -> Vec<Cmd> {
    if changed { vec![Cmd::Redraw] } else { vec![] }
}

#[cfg(test)]
mod tests {
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
        assert!(cmds.is_empty(), "no debería pedir redraw si nada cambió");
    }

    #[test]
    fn full_navigate_and_apply_sequence() {
        let mut m = make_model(3);

        let _ = update(&mut m, Msg::SelectNext);
        let _ = update(&mut m, Msg::SelectNext);
        let cmds = update(&mut m, Msg::Apply);

        assert_eq!(m.picker.selected(), 2);
        match cmds.as_slice() {
            [Cmd::ApplyWallpaper(path)] => {
                assert_eq!(path.file_name().unwrap(), "w2.png");
            }
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
        assert!(
            cmds.is_empty(),
            "fallar no debería salir, está abierto a reintento"
        );
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
        for x_logical in (0..1280).step_by(20) {
            for y_logical in (0..560).step_by(20) {
                if let Some(idx) = m.picker.wallpaper_at(x_logical as f64, y_logical as f64)
                    && idx != 0
                {
                    hit = Some((x_logical as f64, y_logical as f64, idx));
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

        let has_apply = cmds.iter().any(|c| matches!(c, Cmd::ApplyWallpaper(_)));
        let has_redraw = cmds.iter().any(|c| matches!(c, Cmd::Redraw));
        assert!(has_apply && has_redraw);
    }
}
