//! Translate winit keyboard/mouse events into crossterm event types
//! so that EditorState can consume them without knowing which frontend is active.

use crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton as CtMouseButton, MouseEvent,
    MouseEventKind,
};
use winit::event::{ElementState, MouseButton as WinitMouseButton};
use winit::keyboard::{Key, NamedKey};

/// Convert a winit keyboard event into a crossterm KeyEvent.
/// On macOS, Cmd (Super) is mapped to Ctrl so shortcuts work as expected.
pub fn translate_key(
    logical_key: &Key,
    state: ElementState,
    modifiers: winit::keyboard::ModifiersState,
) -> Option<KeyEvent> {
    if state != ElementState::Pressed {
        return None;
    }

    let mut mods = KeyModifiers::empty();
    if modifiers.shift_key() {
        mods |= KeyModifiers::SHIFT;
    }
    // Map both Ctrl and Cmd (Super) to CONTROL so macOS shortcuts work.
    if modifiers.control_key() || modifiers.super_key() {
        mods |= KeyModifiers::CONTROL;
    }
    if modifiers.alt_key() {
        mods |= KeyModifiers::ALT;
    }

    let code = match logical_key {
        Key::Character(s) => {
            let ch = s.chars().next()?;
            KeyCode::Char(ch)
        }
        Key::Named(named) => match named {
            NamedKey::Enter => KeyCode::Enter,
            NamedKey::Escape => KeyCode::Esc,
            NamedKey::Backspace => KeyCode::Backspace,
            NamedKey::Tab => KeyCode::Tab,
            NamedKey::Space => KeyCode::Char(' '),
            NamedKey::Delete => KeyCode::Delete,
            NamedKey::ArrowUp => KeyCode::Up,
            NamedKey::ArrowDown => KeyCode::Down,
            NamedKey::ArrowLeft => KeyCode::Left,
            NamedKey::ArrowRight => KeyCode::Right,
            NamedKey::Home => KeyCode::Home,
            NamedKey::End => KeyCode::End,
            NamedKey::PageUp => KeyCode::PageUp,
            NamedKey::PageDown => KeyCode::PageDown,
            NamedKey::Insert => KeyCode::Insert,
            NamedKey::F1 => KeyCode::F(1),
            NamedKey::F2 => KeyCode::F(2),
            NamedKey::F3 => KeyCode::F(3),
            NamedKey::F4 => KeyCode::F(4),
            NamedKey::F5 => KeyCode::F(5),
            NamedKey::F6 => KeyCode::F(6),
            NamedKey::F7 => KeyCode::F(7),
            NamedKey::F8 => KeyCode::F(8),
            NamedKey::F9 => KeyCode::F(9),
            NamedKey::F10 => KeyCode::F(10),
            NamedKey::F11 => KeyCode::F(11),
            NamedKey::F12 => KeyCode::F(12),
            _ => return None,
        },
        _ => return None,
    };

    Some(KeyEvent::new(code, mods))
}

/// Convert a winit mouse button press/release into a crossterm MouseEvent.
pub fn translate_mouse_button(
    button: WinitMouseButton,
    state: ElementState,
    col: u16,
    row: u16,
    modifiers: winit::keyboard::ModifiersState,
) -> Option<MouseEvent> {
    let btn = match button {
        WinitMouseButton::Left => CtMouseButton::Left,
        WinitMouseButton::Right => CtMouseButton::Right,
        WinitMouseButton::Middle => CtMouseButton::Middle,
        _ => return None,
    };

    let mut mods = KeyModifiers::empty();
    if modifiers.shift_key() {
        mods |= KeyModifiers::SHIFT;
    }
    if modifiers.control_key() {
        mods |= KeyModifiers::CONTROL;
    }
    if modifiers.alt_key() {
        mods |= KeyModifiers::ALT;
    }

    let kind = match state {
        ElementState::Pressed => MouseEventKind::Down(btn),
        ElementState::Released => MouseEventKind::Up(btn),
    };

    Some(MouseEvent { kind, column: col, row, modifiers: mods })
}

/// Build a crossterm scroll event from a winit scroll delta.
pub fn translate_scroll(
    delta_lines: f32,
    col: u16,
    row: u16,
) -> Option<MouseEvent> {
    if delta_lines.abs() < 0.001 {
        return None;
    }
    let kind = if delta_lines > 0.0 {
        MouseEventKind::ScrollUp
    } else {
        MouseEventKind::ScrollDown
    };
    Some(MouseEvent {
        kind,
        column: col,
        row,
        modifiers: KeyModifiers::empty(),
    })
}
