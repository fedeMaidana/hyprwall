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
    layout::{self, Layout},
    render::draw_background,
    style,
    wallpaper::{Wallpaper, apply_wallpaper, current_wallpaper_path, scan_wallpapers},
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
    configured: bool,
    should_close: bool,
    needs_redraw: bool,
    redraw_scheduled: bool,

    keyboard: Option<wl_keyboard::WlKeyboard>,
    keyboard_focus: bool,
    pointer: Option<wl_pointer::WlPointer>,

    wallpapers: Vec<Wallpaper>,
    font: Font,
    selected: usize,
    first_visible: usize,
    hovered: Option<usize>,
    last_layout: Layout,
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

        let selected = initial_selected_wallpaper(&wallpapers);

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
            configured: false,
            should_close: false,
            needs_redraw: false,
            redraw_scheduled: false,

            keyboard: None,
            keyboard_focus: false,
            pointer: None,

            wallpapers,
            font,
            selected,
            first_visible: selected,
            hovered: None,
            last_layout: Layout::empty(),
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
        let width = self.width.max(1);
        let height = self.height.max(1);
        let stride = width as i32 * 4;

        let layout = self.compute_layout();
        self.last_layout = layout.clone();

        let app_width = self.width;
        let app_height = self.height;
        let selected = self.selected;
        let hovered = self.hovered;
        let wallpapers = &self.wallpapers;
        let font = &self.font;
        let wl_surface = self.layer.wl_surface().clone();

        let Ok((buffer, canvas)) = self.pool.create_buffer(
            width as i32,
            height as i32,
            stride,
            wl_shm::Format::Argb8888,
        ) else {
            log::error!("no se pudo crear buffer SHM");
            return;
        };

        canvas.fill(0);

        draw_background(
            canvas, width, height, app_width, app_height, &layout, wallpapers, selected, hovered,
            font,
        );

        wl_surface.damage_buffer(0, 0, width as i32, height as i32);

        if let Err(err) = buffer.attach_to(&wl_surface) {
            log::error!("buffer attach failed: {err:?}");
            return;
        }

        self.layer.commit();
    }

    fn compute_layout(&mut self) -> Layout {
        layout::compute(
            self.width,
            self.height,
            self.wallpapers.len(),
            self.selected,
            &mut self.first_visible,
        )
    }

    fn select_prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        } else {
            self.selected = self.wallpapers.len() - 1;
        }
    }

    fn select_next(&mut self) {
        self.selected = (self.selected + 1) % self.wallpapers.len();
    }

    fn apply_selected(&mut self) {
        let wallpaper = &self.wallpapers[self.selected];

        match apply_wallpaper(&wallpaper.path) {
            Ok(()) => {
                log::info!("wallpaper aplicado: {}", wallpaper.path.display());
                self.should_close = true;
            }
            Err(err) => {
                log::error!("no se pudo aplicar {}: {err:?}", wallpaper.path.display());
            }
        }
    }

    fn wallpaper_at(&self, x: f64, y: f64) -> Option<usize> {
        self.last_layout
            .cards
            .iter()
            .find_map(|(idx, rect)| rect.contains(x, y).then_some(*idx))
    }
}

impl CompositorHandler for AppState {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_factor: i32,
    ) {
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
        let mut changed_selection = false;

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
                self.select_prev();
                changed_selection = true;
            }
            Keysym::Right => {
                self.select_next();
                changed_selection = true;
            }
            _ => {}
        }

        if !changed_selection {
            if let Some(text) = event.utf8.as_deref() {
                match text.to_lowercase().as_str() {
                    "q" => {
                        self.should_close = true;
                        return;
                    }
                    "h" | "a" => {
                        self.select_prev();
                        changed_selection = true;
                    }
                    "l" | "d" => {
                        self.select_next();
                        changed_selection = true;
                    }
                    " " => {
                        self.apply_selected();
                        return;
                    }
                    _ => {}
                }
            }
        }

        if changed_selection {
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
                    let new_hover = self.wallpaper_at(x, y);
                    if self.hovered != new_hover {
                        self.hovered = new_hover;
                        needs_redraw = true;
                    }
                }
                PointerEventKind::Leave { .. } => {
                    self.hovered = None;
                    needs_redraw = true;
                }
                PointerEventKind::Press { button, .. } if button == BTN_LEFT => {
                    if let Some(index) = self.wallpaper_at(x, y) {
                        self.selected = index;
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

fn initial_selected_wallpaper(wallpapers: &[Wallpaper]) -> usize {
    let Some(current_path) = current_wallpaper_path() else {
        return 0;
    };

    let current_path = normalize_path(&current_path);

    wallpapers
        .iter()
        .position(|wallpaper| normalize_path(&wallpaper.path) == current_path)
        .unwrap_or(0)
}

fn normalize_path(path: &std::path::Path) -> std::path::PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}
