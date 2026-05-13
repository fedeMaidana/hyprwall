use anyhow::{Context, Result, bail};
use fontdue::Font;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_pointer,
    delegate_registry, delegate_seat, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{BTN_LEFT, PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};
use std::path::PathBuf;
use wayland_client::{
    Connection, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
};

use crate::{
    font::load_ui_font,
    model::{Cmd, Model, Msg, update},
    picker::Picker,
    render::{build_scene, rasterize},
    style,
    wallpaper::{apply_wallpaper, scan_wallpapers},
};

/// Wayland adapter and Cmd interpreter.
///
/// Holds all the Wayland-specific resources (compositor, layer surface, SHM
/// pool, input devices) plus the [`Model`] and pre-loaded font. Every
/// interesting decision happens in [`update`]; this struct's only jobs are:
///
/// 1. Translate Wayland handler callbacks into [`Msg`]s and feed them to
///    [`update`] via [`Self::dispatch`].
/// 2. Interpret the resulting [`Cmd`]s against the world (paint a frame,
///    set buffer scale, exec swww/hyprpaper).
pub struct AppState {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    _compositor: CompositorState,
    _layer_shell: LayerShell,
    shm: Shm,
    pool: SlotPool,
    layer: LayerSurface,

    redraw_scheduled: bool,
    /// True once we've successfully attached the first buffer to the
    /// surface. Until this flips, `Cmd::Redraw` must paint synchronously:
    /// wlr-layer-shell compositors won't map the surface (or fire `frame`
    /// callbacks) until they see a buffer attached, so scheduling a frame
    /// callback from a buffer-less commit deadlocks the first paint.
    has_rendered: bool,
    should_close: bool,

    keyboard: Option<wl_keyboard::WlKeyboard>,
    keyboard_focus: bool,
    pointer: Option<wl_pointer::WlPointer>,

    model: Model,
    font: Font,
}

impl AppState {
    pub fn run(wallpaper_dir: PathBuf) -> Result<()> {
        let wallpapers = scan_wallpapers(&wallpaper_dir).with_context(|| {
            format!(
                "no se pudieron cargar wallpapers desde {}",
                wallpaper_dir.display()
            )
        })?;

        if wallpapers.is_empty() {
            bail!("no encontré imágenes en {}", wallpaper_dir.display());
        }

        log::info!("loaded {} wallpapers", wallpapers.len());

        let picker = Picker::with_current_wallpaper(wallpapers);
        let model = Model::new(
            picker,
            style::surface::WIDTH_HINT,
            style::surface::HEIGHT_HINT,
        );
        let font = load_ui_font()?;

        let conn = Connection::connect_to_env().context("no se pudo conectar a Wayland")?;
        let (globals, mut event_queue) =
            registry_queue_init::<AppState>(&conn).context("registry_queue_init failed")?;
        let qh = event_queue.handle();

        let compositor =
            CompositorState::bind(&globals, &qh).context("wl_compositor no disponible")?;
        let layer_shell = LayerShell::bind(&globals, &qh)
            .context("zwlr_layer_shell_v1 no disponible; Hyprland debería soportarlo")?;
        let shm = Shm::bind(&globals, &qh).context("wl_shm no disponible")?;

        let surface = compositor.create_surface(&qh);
        let layer =
            layer_shell.create_layer_surface(&qh, surface, Layer::Overlay, Some("hyprwall"), None);

        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        layer.set_exclusive_zone(-1);
        layer.set_size(0, 0);
        layer.commit();

        let pool = SlotPool::new(
            (style::surface::WIDTH_HINT * style::surface::HEIGHT_HINT * 4) as usize,
            &shm,
        )
        .context("no se pudo crear wl_shm SlotPool")?;

        let mut app = Self {
            registry_state: RegistryState::new(&globals),
            seat_state: SeatState::new(&globals, &qh),
            output_state: OutputState::new(&globals, &qh),
            _compositor: compositor,
            _layer_shell: layer_shell,
            shm,
            pool,
            layer,

            redraw_scheduled: false,
            has_rendered: false,
            should_close: false,

            keyboard: None,
            keyboard_focus: false,
            pointer: None,

            model,
            font,
        };

        while !app.model.configured {
            event_queue
                .blocking_dispatch(&mut app)
                .context("dispatch esperando configure")?;
        }

        while !app.should_close {
            event_queue
                .blocking_dispatch(&mut app)
                .context("event_queue dispatch")?;
        }

        Ok(())
    }

    /// The single funnel for state changes. Every Wayland handler converts
    /// its callback into a [`Msg`] and routes it through here. Multiple
    /// `Msg`s can be dispatched in sequence to drain Cmd chains (e.g.
    /// `ApplyWallpaper` → `WallpaperApplied` → `Exit`).
    fn dispatch(&mut self, qh: &QueueHandle<Self>, msg: Msg) {
        let mut pending: Vec<Msg> = vec![msg];

        while let Some(msg) = pending.pop() {
            log::trace!("msg: {msg:?}");
            let cmds = update(&mut self.model, msg);
            for cmd in cmds {
                log::trace!("cmd: {cmd:?}");
                if let Some(followup) = self.execute(qh, cmd) {
                    pending.push(followup);
                }
            }
        }
    }

    /// Interpret a single [`Cmd`] against the world. Returns an optional
    /// follow-up [`Msg`] (e.g. completion of `ApplyWallpaper`).
    fn execute(&mut self, qh: &QueueHandle<Self>, cmd: Cmd) -> Option<Msg> {
        match cmd {
            Cmd::Redraw => {
                // First frame must be synchronous: the compositor needs a
                // buffer attached before it'll map the layer surface or
                // emit frame callbacks. Subsequent redraws schedule via
                // the frame callback to coalesce with the refresh rate.
                if self.has_rendered {
                    self.request_redraw(qh);
                } else {
                    self.render_now();
                }
                None
            }
            Cmd::SetBufferScale(scale) => {
                self.layer.wl_surface().set_buffer_scale(scale);
                None
            }
            Cmd::ApplyWallpaper(path) => {
                // Sync today. If this ever becomes async, run it on a thread
                // and let the thread post WallpaperApplied/Failed via the
                // event queue; the rest of MVU doesn't need to change.
                Some(match apply_wallpaper(&path) {
                    Ok(()) => Msg::WallpaperApplied(path),
                    Err(err) => Msg::WallpaperFailed {
                        path,
                        error: format!("{err:#}"),
                    },
                })
            }
            Cmd::Exit => {
                self.should_close = true;
                None
            }
        }
    }

    fn request_redraw(&mut self, qh: &QueueHandle<Self>) {
        if self.redraw_scheduled {
            return;
        }
        self.redraw_scheduled = true;

        let wl_surface = self.layer.wl_surface().clone();
        wl_surface.frame(qh, wl_surface.clone());
        self.layer.commit();
    }

    fn render_now(&mut self) {
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

impl CompositorHandler for AppState {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        new_factor: i32,
    ) {
        if self.layer.wl_surface() != surface {
            return;
        }
        self.dispatch(qh, Msg::ScaleChanged(new_factor));
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        self.redraw_scheduled = false;
        self.render_now();
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for AppState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for AppState {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {
        self.should_close = true;
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        let (w, h) = configure.new_size;
        let width = if w == 0 {
            style::surface::WIDTH_HINT
        } else {
            w
        };
        let height = if h == 0 {
            style::surface::HEIGHT_HINT
        } else {
            h
        };

        self.dispatch(qh, Msg::Configured { width, height });
    }
}

impl SeatHandler for AppState {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            match self.seat_state.get_keyboard(qh, &seat, None) {
                Ok(keyboard) => self.keyboard = Some(keyboard),
                Err(err) => log::warn!("no se pudo crear keyboard: {err:?}"),
            }
        }
        if capability == Capability::Pointer && self.pointer.is_none() {
            match self.seat_state.get_pointer(qh, &seat) {
                Ok(pointer) => self.pointer = Some(pointer),
                Err(err) => log::warn!("no se pudo crear pointer: {err:?}"),
            }
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard {
            if let Some(keyboard) = self.keyboard.take() {
                keyboard.release();
            }
        }
        if capability == Capability::Pointer {
            if let Some(pointer) = self.pointer.take() {
                pointer.release();
            }
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for AppState {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
        if self.layer.wl_surface() == surface {
            self.keyboard_focus = true;
        }
    }

    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _: u32,
    ) {
        if self.layer.wl_surface() == surface {
            self.keyboard_focus = false;
        }
    }

    fn press_key(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        if let Some(msg) = key_event_to_msg(&event) {
            self.dispatch(qh, msg);
        }
    }

    fn repeat_key(
        &mut self,
        conn: &Connection,
        qh: &QueueHandle<Self>,
        keyboard: &wl_keyboard::WlKeyboard,
        serial: u32,
        event: KeyEvent,
    ) {
        self.press_key(conn, qh, keyboard, serial, event);
    }

    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _serial: u32,
        _modifiers: Modifiers,
        _raw_modifiers: RawModifiers,
        _layout: u32,
    ) {
    }
}

impl PointerHandler for AppState {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            if &event.surface != self.layer.wl_surface() {
                continue;
            }

            let (x, y) = event.position;

            match event.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    self.dispatch(qh, Msg::HoverAt { x, y });
                }
                PointerEventKind::Leave { .. } => {
                    self.dispatch(qh, Msg::ClearHover);
                }
                PointerEventKind::Press { button, .. } if button == BTN_LEFT => {
                    self.dispatch(qh, Msg::PointerPressedAt { x, y });
                }
                _ => {}
            }
        }
    }
}

impl ShmHandler for AppState {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for AppState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_compositor!(AppState);
delegate_output!(AppState);
delegate_shm!(AppState);
delegate_seat!(AppState);
delegate_keyboard!(AppState);
delegate_pointer!(AppState);
delegate_layer!(AppState);
delegate_registry!(AppState);

fn key_event_to_msg(event: &KeyEvent) -> Option<Msg> {
    match event.keysym {
        Keysym::Escape => return Some(Msg::Quit),
        Keysym::Return => return Some(Msg::Apply),
        Keysym::Left => return Some(Msg::SelectPrev),
        Keysym::Right => return Some(Msg::SelectNext),
        _ => {}
    }

    let text = event.utf8.as_deref()?;
    match text.to_lowercase().as_str() {
        "q" => Some(Msg::Quit),
        " " => Some(Msg::Apply),
        "h" | "a" => Some(Msg::SelectPrev),
        "l" | "d" => Some(Msg::SelectNext),
        _ => None,
    }
}
