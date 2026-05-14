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
#[path = "../tests/unit/model.rs"]
mod tests;
