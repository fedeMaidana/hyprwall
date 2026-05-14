use std::path::{Path, PathBuf};

use crate::{
    layout::{self, Layout},
    wallpaper::{Wallpaper, current_wallpaper_path},
};

pub struct Picker {
    wallpapers: Vec<Wallpaper>,
    selected: usize,
    hovered: Option<usize>,
    last_layout: Layout,
}

impl Picker {
    pub fn new(wallpapers: Vec<Wallpaper>, initial_selected: usize) -> Self {
        let selected = if wallpapers.is_empty() {
            0
        } else {
            initial_selected.min(wallpapers.len() - 1)
        };
        Self {
            wallpapers,
            selected,
            hovered: None,
            last_layout: Layout::empty(),
        }
    }

    pub fn with_current_wallpaper(wallpapers: Vec<Wallpaper>) -> Self {
        let initial = find_initial_selected(&wallpapers);
        Self::new(wallpapers, initial)
    }

    pub fn wallpapers(&self) -> &[Wallpaper] {
        &self.wallpapers
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn hovered(&self) -> Option<usize> {
        self.hovered
    }
    pub fn current(&self) -> &Wallpaper {
        &self.wallpapers[self.selected]
    }

    pub fn recompute_layout(&mut self, width: u32, height: u32) -> &Layout {
        self.last_layout = layout::compute(width, height, self.wallpapers.len(), self.selected);
        &self.last_layout
    }

    pub fn select_prev(&mut self) -> bool {
        if self.wallpapers.is_empty() {
            return false;
        }
        let next = if self.selected == 0 {
            self.wallpapers.len() - 1
        } else {
            self.selected - 1
        };
        self.set_selected(next)
    }

    pub fn select_next(&mut self) -> bool {
        if self.wallpapers.is_empty() {
            return false;
        }
        self.set_selected((self.selected + 1) % self.wallpapers.len())
    }

    pub fn select_index(&mut self, index: usize) -> bool {
        if index >= self.wallpapers.len() {
            return false;
        }
        self.set_selected(index)
    }

    fn set_selected(&mut self, index: usize) -> bool {
        if index == self.selected {
            return false;
        }
        self.selected = index;
        true
    }

    pub fn hover_at(&mut self, x: f64, y: f64) -> bool {
        let new_hover = self.wallpaper_at(x, y);
        if self.hovered != new_hover {
            self.hovered = new_hover;
            true
        } else {
            false
        }
    }

    pub fn clear_hover(&mut self) -> bool {
        if self.hovered.is_some() {
            self.hovered = None;
            true
        } else {
            false
        }
    }

    pub(crate) fn wallpaper_at(&self, x: f64, y: f64) -> Option<usize> {
        self.last_layout
            .cards
            .iter()
            .find_map(|(idx, rect)| rect.contains(x, y).then_some(*idx))
    }
}

fn find_initial_selected(wallpapers: &[Wallpaper]) -> usize {
    let Some(current_path) = current_wallpaper_path() else {
        return 0;
    };
    let current_path = normalize_path(&current_path);
    wallpapers
        .iter()
        .position(|w| normalize_path(&w.path) == current_path)
        .unwrap_or(0)
}

fn normalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
#[path = "../tests/unit/picker.rs"]
mod tests;
