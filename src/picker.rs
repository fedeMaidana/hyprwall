use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::{
    layout::{self, Layout},
    style,
    wallpaper::{Wallpaper, current_wallpaper_path},
};

pub struct Picker {
    wallpapers: Vec<Wallpaper>,
    /// Posición animada del carrusel en "espacio de índices" (2.5 = a
    /// mitad de camino entre la card 2 y la 3). Sin acotar; se envuelve
    /// al leer y al asentarse la animación.
    pos: f32,
    /// A dónde está yendo `pos`. La selección lógica es round(target).
    target: f32,
    /// Velocidad actual del resorte (cards/s). Se conserva entre
    /// retargets: encadenar scrolls fluye en vez de sacudir.
    vel: f32,
    last_tick: Option<Instant>,
    hovered: Option<usize>,
    applied: Option<usize>,
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
            pos: selected as f32,
            target: selected as f32,
            vel: 0.0,
            last_tick: None,
            hovered: None,
            applied: None,
            last_layout: Layout::empty(),
        }
    }

    pub fn with_current_wallpaper(wallpapers: Vec<Wallpaper>) -> Self {
        let applied = find_current_index(&wallpapers);
        let mut picker = Self::new(wallpapers, applied.unwrap_or(0));
        picker.applied = applied;
        picker
    }

    pub fn wallpapers(&self) -> &[Wallpaper] {
        &self.wallpapers
    }
    pub fn selected(&self) -> usize {
        let count = self.wallpapers.len();
        if count == 0 {
            return 0;
        }
        let wrapped = self.target.round().rem_euclid(count as f32) as usize;
        wrapped.min(count - 1)
    }
    pub fn hovered(&self) -> Option<usize> {
        self.hovered
    }
    pub fn applied(&self) -> Option<usize> {
        self.applied
    }
    pub fn current(&self) -> &Wallpaper {
        &self.wallpapers[self.selected()]
    }

    pub fn recompute_layout(&mut self, width: u32, height: u32) -> &Layout {
        self.last_layout = layout::compute(width, height, self.wallpapers.len(), self.pos);
        &self.last_layout
    }

    /// ¿La posición sigue persiguiendo al objetivo?
    pub fn is_animating(&self) -> bool {
        (self.target - self.pos).abs() > style::scroll::SNAP_EPS
            || self.vel.abs() > style::scroll::SNAP_VEL_EPS
    }

    /// Avanza un paso de animación. Resorte críticamente amortiguado con
    /// solución exacta: arranca suave desde velocidad cero, acelera y
    /// asienta sin rebote. Devuelve true si hace falta otro frame.
    pub fn tick(&mut self) -> bool {
        if self.wallpapers.is_empty() {
            return false;
        }
        if !self.is_animating() {
            self.last_tick = None;
            return false;
        }

        let now = Instant::now();
        let dt = self
            .last_tick
            .map(|t| now.duration_since(t).as_secs_f32())
            .unwrap_or(0.0)
            .min(style::scroll::MAX_FRAME_DT);
        self.last_tick = Some(now);

        if dt > 0.0 {
            // x(t) = target + (a + b·t)·e^(−ω·t), con a = pos − target y
            // b = vel + ω·a. Integración exacta: estable con cualquier dt.
            let omega = style::scroll::SPRING_OMEGA;
            let e = (-omega * dt).exp();
            let a = self.pos - self.target;
            let b = self.vel + omega * a;
            self.pos = self.target + (a + b * dt) * e;
            self.vel = (self.vel - omega * b * dt) * e;
        }

        if !self.is_animating() {
            self.settle();
            return false;
        }
        true
    }

    /// Asienta la animación y normaliza pos/target a [0, count).
    fn settle(&mut self) {
        let count = self.wallpapers.len() as f32;
        self.pos = self.target.rem_euclid(count);
        self.target = self.pos;
        self.vel = 0.0;
        self.last_tick = None;
    }

    pub fn select_prev(&mut self) -> bool {
        self.nudge(-1.0)
    }

    pub fn select_next(&mut self) -> bool {
        self.nudge(1.0)
    }

    fn nudge(&mut self, delta: f32) -> bool {
        if self.wallpapers.len() < 2 {
            return false;
        }
        self.target = self.target.round() + delta;
        true
    }

    /// Scroll de rueda/touchpad medido en clicks de rueda (fraccional
    /// para fuentes continuas).
    pub fn scroll_by(&mut self, notches: f32) -> bool {
        if self.wallpapers.len() < 2 || notches == 0.0 {
            return false;
        }
        self.target += notches * style::scroll::WHEEL_CARDS_PER_NOTCH;
        true
    }

    pub fn select_index(&mut self, index: usize) -> bool {
        let count = self.wallpapers.len();
        if index >= count {
            return false;
        }
        if index == self.selected() {
            return false;
        }

        // Camino más corto respetando el wrap del carrusel.
        let count = count as f32;
        let anchor = self.target.round();
        let mut delta = (index as f32 - anchor).rem_euclid(count);
        if delta >= count / 2.0 {
            delta -= count;
        }
        self.target = anchor + delta;
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
            .find_map(|card| card.rect.contains(x, y).then_some(card.index))
    }
}

fn find_current_index(wallpapers: &[Wallpaper]) -> Option<usize> {
    let current_path = normalize_path(&current_wallpaper_path()?);

    wallpapers
        .iter()
        .position(|w| normalize_path(&w.path) == current_path)
}

fn normalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
#[path = "../tests/unit/picker.rs"]
mod tests;
