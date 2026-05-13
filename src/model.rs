//! MVU (Model-View-Update) core.
//!
//! - [`Model`] holds the entire application state.
//! - [`Msg`] enumerates every event the system can react to.
//! - [`Cmd`] enumerates every side effect [`update`] can request.
//!
//! [`update`] is `(&mut Model, Msg) -> Vec<Cmd>`: mutates state and returns
//! a list of effects. Wayland glue in `app.rs` is the only place that
//! translates handler callbacks into [`Msg`] and actually executes [`Cmd`].
//! The view side is `render::build_scene`, also pure.
//!
//! This is intentionally pragmatic rather than purist Elm: `update` takes
//! `&mut Model` for zero-clone performance. Tests still benefit because
//! everything below `app.rs` is reachable without Wayland.

use std::path::PathBuf;

use crate::picker::Picker;

/// Every event the system can react to. Wayland handlers, the [`Cmd`]
/// interpreter (for completion of side effects), and tests are the only
/// producers of `Msg`.
#[derive(Debug, Clone)]
pub enum Msg {
    // Carousel navigation.
    SelectPrev,
    SelectNext,
    SelectIndex(usize),

    // Pointer hit-testing.
    HoverAt { x: f64, y: f64 },
    ClearHover,

    // Intentional actions.
    Apply,
    Quit,

    // Wayland surface lifecycle.
    Configured { width: u32, height: u32 },
    ScaleChanged(i32),

    // Completion of side effects.
    WallpaperApplied(PathBuf),
    WallpaperFailed { path: PathBuf, error: String },
}

/// Every side effect [`update`] can request. The interpreter in `app.rs`
/// is responsible for executing these against the world (Wayland, the
/// applier, the OS). Effects that produce results feed them back as new
/// [`Msg`]s.
#[derive(Debug)]
pub enum Cmd {
    /// Request a frame from the compositor and redraw on it.
    Redraw,
    /// Spawn the apply-wallpaper process for the given path. Sync today;
    /// could become async without touching `Model` / `update`.
    ApplyWallpaper(PathBuf),
    /// Propagate a new buffer scale to the Wayland surface.
    SetBufferScale(i32),
    /// Tear down the event loop.
    Exit,
}

/// Full application state. Composes [`Picker`] (carousel domain) with
/// Wayland-visible bits.
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

/// Heart of MVU: take an event, mutate the model, return any side effects.
pub fn update(model: &mut Model, msg: Msg) -> Vec<Cmd> {
    match msg {
        Msg::SelectPrev => redraw_if(model.picker.select_prev()),
        Msg::SelectNext => redraw_if(model.picker.select_next()),
        Msg::SelectIndex(idx) => redraw_if(model.picker.select_index(idx)),

        Msg::HoverAt { x, y } => redraw_if(model.picker.hover_at(x, y)),
        Msg::ClearHover => redraw_if(model.picker.clear_hover()),

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
            // No salimos: el usuario puede intentar otro wallpaper.
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

        // ArrowRight, ArrowRight, Enter.
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

        // Side effect completion fires Exit.
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

        // First configure transitions configured=true.
        let cmds = update(
            &mut m,
            Msg::Configured {
                width: 1920,
                height: 1080,
            },
        );
        assert!(has_redraw(&cmds));

        // Same size again, already configured: no redraw.
        let cmds = update(
            &mut m,
            Msg::Configured {
                width: 1920,
                height: 1080,
            },
        );
        assert!(cmds.is_empty());

        // Different size: redraw.
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
    fn click_via_select_index_then_apply() {
        // Lo que hace pointer_frame en Press: SelectIndex(idx) + Apply.
        let mut m = make_model(5);
        let _ = update(&mut m, Msg::SelectIndex(3));
        let cmds = update(&mut m, Msg::Apply);
        match cmds.as_slice() {
            [Cmd::ApplyWallpaper(path)] => {
                assert_eq!(path.file_name().unwrap(), "w3.png");
            }
            other => panic!("got {other:?}"),
        }
    }
}
