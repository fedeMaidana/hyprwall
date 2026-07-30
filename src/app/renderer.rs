//! Frame scheduling and SHM buffer rendering.
//!
//! Responsible for: allocating a physical-resolution SHM buffer, invoking
//! the render pipeline, and scheduling frame callbacks with the compositor.

use smithay_client_toolkit::shell::WaylandSurface;
use wayland_client::{QueueHandle, protocol::wl_shm};

use crate::render::{build_scene, rasterize};

use super::AppState;

impl AppState {
    /// Schedules a redraw via a Wayland frame callback. No-op if a frame
    /// is already pending.
    pub(super) fn request_redraw(&mut self, qh: &QueueHandle<AppState>) {
        if self.redraw_scheduled {
            return;
        }
        self.redraw_scheduled = true;

        let wl_surface = self.layer.wl_surface().clone();
        wl_surface.frame(qh, wl_surface.clone());
        self.layer.commit();
    }

    /// Allocates a physical-resolution SHM buffer, rasterizes the current
    /// scene, and commits it to the Wayland surface.
    pub(super) fn render_now(&mut self) {
        let logical_w = self.model.logical_width.max(1);
        let logical_h = self.model.logical_height.max(1);

        let scale = self.model.scale.max(1) as u32;
        let phys_w = logical_w * scale;
        let phys_h = logical_h * scale;
        let stride = phys_w as i32 * 4;

        let layout = self
            .model
            .picker
            .recompute_layout(logical_w, logical_h)
            .clone();

        let scene = build_scene(
            logical_w,
            logical_h,
            &layout,
            self.model.picker.wallpapers(),
            self.model.picker.selected(),
            self.model.picker.hovered(),
            self.panel_color,
        );

        let wl_surface = self.layer.wl_surface().clone();

        let Ok((buffer, canvas)) = self.pool.create_buffer(
            phys_w as i32,
            phys_h as i32,
            stride,
            wl_shm::Format::Argb8888,
        ) else {
            log::error!("no se pudo crear buffer SHM");
            return;
        };

        canvas.fill(0);
        rasterize(canvas, phys_w, phys_h, scale as f32, &scene, &self.font);

        wl_surface.damage_buffer(0, 0, phys_w as i32, phys_h as i32);

        if let Err(err) = buffer.attach_to(&wl_surface) {
            log::error!("buffer attach failed: {err:?}");
            return;
        }

        self.layer.commit();
        self.has_rendered = true;
    }
}
