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
    picker::Picker,
    render::{build_scene, rasterize},
    style,
    wallpaper::scan_wallpapers,
};

pub struct AppState {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    _compositor: CompositorState,
    _layer_shell: LayerShell,
    shm: Shm,
    pool: SlotPool,
    layer: LayerSurface,

    width: u32,
    height: u32,
    /// HiDPI buffer scale factor. 1 = standard, 2 = retina, etc.
    /// Updated when the surface enters an output via
    /// `CompositorHandler::scale_factor_changed`.
    scale: i32,
    configured: bool,
    should_close: bool,
    needs_redraw: bool,
    redraw_scheduled: bool,

    keyboard: Option<wl_keyboard::WlKeyboard>,
    keyboard_focus: bool,
    pointer: Option<wl_pointer::WlPointer>,

    picker: Picker,
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

        // 0, 0 + anchors en todos los lados = el compositor decide el tamaño,
        // normalmente pantalla completa.
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

            width: style::surface::WIDTH_HINT,
            height: style::surface::HEIGHT_HINT,
            scale: 1,
            configured: false,
            should_close: false,
            needs_redraw: false,
            redraw_scheduled: false,

            keyboard: None,
            keyboard_focus: false,
            pointer: None,

            picker,
            font,
        };

        while !app.configured {
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

    fn request_redraw(&mut self, qh: &QueueHandle<Self>) {
        self.needs_redraw = true;

        if self.redraw_scheduled {
            return;
        }

        self.redraw_scheduled = true;

        let wl_surface = self.layer.wl_surface().clone();
        wl_surface.frame(qh, wl_surface.clone());
        self.layer.commit();
    }

    fn render_now(&mut self) {
        // Tamaño lógico (lo que reporta el compositor en configure).
        let logical_w = self.width.max(1);
        let logical_h = self.height.max(1);

        // Tamaño físico del buffer SHM. El compositor escala el buffer al
        // viewport del output respetando este factor; pintar al tamaño físico
        // evita la borrosidad de que el compositor escale un buffer 1×.
        let scale = self.scale.max(1) as u32;
        let phys_w = logical_w * scale;
        let phys_h = logical_h * scale;
        let stride = phys_w as i32 * 4;

        let layout = self.picker.recompute_layout(logical_w, logical_h).clone();

        let scene = build_scene(
            logical_w,
            logical_h,
            &layout,
            self.picker.wallpapers(),
            self.picker.selected(),
            self.picker.hovered(),
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

        // damage_buffer expresa la región dañada en coordenadas de buffer
        // físico, no lógico.
        wl_surface.damage_buffer(0, 0, phys_w as i32, phys_h as i32);

        if let Err(err) = buffer.attach_to(&wl_surface) {
            log::error!("buffer attach failed: {err:?}");
            return;
        }

        self.layer.commit();
    }

    fn apply_selected(&mut self) {
        let current_path = self.picker.current().path.clone();

        match self.picker.apply_current() {
            Ok(()) => {
                log::info!("wallpaper aplicado: {}", current_path.display());
                self.should_close = true;
            }
            Err(err) => {
                log::error!("no se pudo aplicar {}: {err:?}", current_path.display());
            }
        }
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
        if self.scale == new_factor || new_factor < 1 {
            return;
        }

        log::info!("HiDPI scale changed: {} -> {new_factor}", self.scale);
        self.scale = new_factor;
        surface.set_buffer_scale(new_factor);

        if self.configured {
            self.request_redraw(qh);
        }
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

        if !self.needs_redraw {
            return;
        }

        self.needs_redraw = false;
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

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
}

impl LayerShellHandler for AppState {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {
        self.should_close = true;
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        let (w, h) = configure.new_size;
        self.width = if w == 0 {
            style::surface::WIDTH_HINT
        } else {
            w
        };
        self.height = if h == 0 {
            style::surface::HEIGHT_HINT
        } else {
            h
        };

        self.configured = true;

        self.needs_redraw = false;
        self.redraw_scheduled = false;
        self.render_now();
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
        _keysyms: &[Keysym],
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
        let mut changed = false;

        match event.keysym {
            Keysym::Escape => {
                self.should_close = true;
                return;
            }
            Keysym::Return => {
                self.apply_selected();
                return;
            }
            Keysym::Left => {
                changed = self.picker.select_prev();
            }
            Keysym::Right => {
                changed = self.picker.select_next();
            }
            _ => {}
        }

        if !changed {
            if let Some(text) = event.utf8.as_deref() {
                match text.to_lowercase().as_str() {
                    "q" => {
                        self.should_close = true;
                        return;
                    }
                    "h" | "a" => {
                        changed = self.picker.select_prev();
                    }
                    "l" | "d" => {
                        changed = self.picker.select_next();
                    }
                    " " => {
                        self.apply_selected();
                        return;
                    }
                    _ => {}
                }
            }
        }

        if changed {
            self.request_redraw(qh);
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
        let mut needs_redraw = false;

        for event in events {
            if &event.surface != self.layer.wl_surface() {
                continue;
            }

            let (x, y) = event.position;

            match event.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    if self.picker.hover_at(x, y) {
                        needs_redraw = true;
                    }
                }
                PointerEventKind::Leave { .. } => {
                    if self.picker.clear_hover() {
                        needs_redraw = true;
                    }
                }
                PointerEventKind::Press { button, .. } if button == BTN_LEFT => {
                    if let Some(index) = self.picker.wallpaper_at(x, y) {
                        self.picker.select_index(index);
                        self.apply_selected();
                    }
                }
                _ => {}
            }
        }

        if needs_redraw {
            self.request_redraw(qh);
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
