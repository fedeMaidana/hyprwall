//! Input translation for [`AppState`]: keyboard and pointer events → [`Msg`].
//!
//! `key_event_to_msg` is a pure function (testable without Wayland). The
//! trait impls delegate to it and to `dispatch`; no decisions are made here.

use smithay_client_toolkit::{
    seat::keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
    seat::pointer::{BTN_LEFT, PointerEvent, PointerEventKind, PointerHandler},
    shell::WaylandSurface,
};
use wayland_client::{
    Connection, QueueHandle,
    protocol::{wl_keyboard, wl_pointer, wl_surface},
};

use crate::{app::AppState, model::Msg};

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

/// Pure translation from a Wayland key event to a [`Msg`]. `None` means
/// the key is ignored. Lives here so it's testable without Wayland.
pub(super) fn key_event_to_msg(event: &KeyEvent) -> Option<Msg> {
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
