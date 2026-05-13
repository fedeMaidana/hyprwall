use std::path::{Path, PathBuf};

use crate::{
    layout::{self, Layout},
    wallpaper::{Wallpaper, current_wallpaper_path},
};

/// Pure domain state of the wallpaper carousel: which wallpapers we have,
/// which one is selected, which one is hovered, and the last computed layout
/// for hit-testing. No Wayland here.
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

    /// Builds a Picker selecting the wallpaper that matches the currently
    /// applied one (via swww/hyprpaper/cache), falling back to index 0.
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

    /// Recomputes the carousel layout for the given surface size and stores
    /// it. Returns a reference to the new layout.
    pub fn recompute_layout(&mut self, width: u32, height: u32) -> &Layout {
        self.last_layout = layout::compute(width, height, self.wallpapers.len(), self.selected);
        &self.last_layout
    }

    /// Moves selection to the previous wallpaper (wraps). Returns true iff
    /// the selection actually changed.
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

    /// Moves selection to the next wallpaper (wraps). Returns true iff
    /// the selection actually changed.
    pub fn select_next(&mut self) -> bool {
        if self.wallpapers.is_empty() {
            return false;
        }

        let next = (self.selected + 1) % self.wallpapers.len();
        self.set_selected(next)
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

    /// Updates hover state from pointer coordinates against the last computed
    /// layout. Returns true iff the hover changed.
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
        .position(|wallpaper| normalize_path(&wallpaper.path) == current_path)
        .unwrap_or(0)
}

fn normalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
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
        assert!(!p.select_index(0)); // already selected
        assert!(p.select_index(2));
        assert!(!p.select_index(2));
        assert_eq!(p.selected(), 2);
    }

    #[test]
    fn clear_hover_only_signals_when_was_set() {
        let mut p = make_picker(2);
        assert!(!p.clear_hover()); // ya está en None

        p.hovered = Some(0);
        assert!(p.clear_hover());
        assert_eq!(p.hovered(), None);
    }
}
