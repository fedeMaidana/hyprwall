//! Frame scheduling and SHM buffer rendering.
//!
//! Responsible for: reusing physical-resolution SHM buffers, invoking
//! the render pipeline, and scheduling frame callbacks with the compositor.

use smithay_client_toolkit::{shell::WaylandSurface, shm::slot::Buffer};
use wayland_client::{QueueHandle, protocol::wl_shm};

use crate::{
    geometry::Rect,
    render::{Selection, build_scene},
};

use super::AppState;

const MAX_FRAME_BUFFERS: usize = 3;

pub(super) struct FrameBuffer {
    buffer: Buffer,
    width: u32,
    height: u32,
    generation: Option<u64>,
}

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

    /// Updates a released SHM buffer and commits it to the Wayland surface.
    /// Returns false when presentation must be retried (including backpressure).
    pub(super) fn render_now(&mut self) -> bool {
        let logical_w = self.model.logical_width.max(1);
        let logical_h = self.model.logical_height.max(1);

        let scale = self.model.scale.max(1) as u32;
        let phys_w = logical_w * scale;
        let phys_h = logical_h * scale;
        let stride = phys_w as i32 * 4;

        // Dropping an active SCTK Buffer defers its destruction until release.
        // Never write to a buffer while the compositor is still reading it.
        self.frame_buffers
            .retain(|cached| cached.width == phys_w && cached.height == phys_h);
        let buffer_index = match self
            .frame_buffers
            .iter()
            .position(|cached| self.pool.canvas(&cached.buffer).is_some())
        {
            Some(index) => index,
            None if self.frame_buffers.len() < MAX_FRAME_BUFFERS => {
                let Ok((buffer, _)) = self.pool.create_buffer(
                    phys_w as i32,
                    phys_h as i32,
                    stride,
                    wl_shm::Format::Argb8888,
                ) else {
                    log::error!("no se pudo crear buffer SHM");
                    return false;
                };
                self.frame_buffers.push(FrameBuffer {
                    buffer,
                    width: phys_w,
                    height: phys_h,
                    generation: None,
                });
                self.frame_buffers.len() - 1
            }
            None => return false,
        };

        // Avanza la animación del carrusel antes de posicionar las cards.
        self.model.picker.tick();

        let layout = self
            .model
            .picker
            .recompute_layout(logical_w, logical_h)
            .clone();

        let selection = Selection {
            selected: self.model.picker.selected(),
            hovered: self.model.picker.hovered(),
            applied: self.model.picker.applied(),
        };

        let scene = build_scene(
            logical_w,
            logical_h,
            &layout,
            self.model.picker.wallpapers(),
            selection,
            self.colors,
        );

        let wl_surface = self.layer.wl_surface().clone();

        let Some(frame) = self
            .rasterizer
            .render(phys_w, phys_h, scale as f32, &scene, &self.font)
        else {
            return false;
        };

        let cached = &mut self.frame_buffers[buffer_index];
        let Some(canvas) = self.pool.canvas(&cached.buffer) else {
            return false;
        };
        frame.copy_to(canvas, cached.generation != Some(frame.generation));
        cached.generation = Some(frame.generation);

        let damage = if self.presented_generation == Some(frame.generation) {
            frame.damage
        } else {
            Rect {
                x: 0,
                y: 0,
                w: phys_w as i32,
                h: phys_h as i32,
            }
        };
        wl_surface.damage_buffer(damage.x, damage.y, damage.w, damage.h);

        if let Err(err) = cached.buffer.attach_to(&wl_surface) {
            log::error!("buffer attach failed: {err:?}");
            return false;
        }

        // Commit scale and matching buffer together. Set this after a
        // successful attach so an error cannot leave a scale change pending
        // for request_redraw's bufferless commit.
        wl_surface.set_buffer_scale(scale as i32);
        self.layer.commit();
        self.presented_generation = Some(frame.generation);
        self.has_rendered = true;
        true
    }
}
