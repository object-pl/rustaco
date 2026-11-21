//! Keys from winit to the keymap's names (the DOM's `key` values, lowercased).

use rustaco_core::KeyPress;
use winit::keyboard::{Key, ModifiersState, NamedKey};

/// The keymap's view of a key press: `key` is the key without Shift applied (so a binding
/// for `ctrl+shift+[` matches the `[` key on layouts where Shift makes it `{`), lowercased.
/// None for modifier keys alone.
pub fn key_press(key_without_modifiers: &Key, modifiers: ModifiersState) -> Option<KeyPress> {
    let name = match key_without_modifiers {
        Key::Character(s) => s.to_lowercase(),
        Key::Named(named) => named_key(*named)?.to_owned(),
        _ => return None,
    };
    Some(KeyPress {
        ctrl: modifiers.control_key(),
        shift: modifiers.shift_key(),
        alt: modifiers.alt_key(),
        meta: modifiers.super_key(),
        key: name,
    })
}

fn named_key(key: NamedKey) -> Option<&'static str> {
    Some(match key {
        NamedKey::ArrowUp => "arrowup",
        NamedKey::ArrowDown => "arrowdown",
        NamedKey::ArrowLeft => "arrowleft",
        NamedKey::ArrowRight => "arrowright",
        NamedKey::Enter => "enter",
        NamedKey::Tab => "tab",
        NamedKey::Space => " ",
        NamedKey::Backspace => "backspace",
        NamedKey::Delete => "delete",
        NamedKey::Escape => "escape",
        NamedKey::Home => "home",
        NamedKey::End => "end",
        NamedKey::PageUp => "pageup",
        NamedKey::PageDown => "pagedown",
        NamedKey::Insert => "insert",
        NamedKey::F1 => "f1",
        NamedKey::F2 => "f2",
        NamedKey::F3 => "f3",
        NamedKey::F4 => "f4",
        NamedKey::F5 => "f5",
        NamedKey::F6 => "f6",
        NamedKey::F7 => "f7",
        NamedKey::F8 => "f8",
        NamedKey::F9 => "f9",
        NamedKey::F10 => "f10",
        NamedKey::F11 => "f11",
        NamedKey::F12 => "f12",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_the_keymap() {
        let p = key_press(&Key::Named(NamedKey::ArrowLeft), ModifiersState::CONTROL).unwrap();
        assert_eq!((p.key.as_str(), p.ctrl, p.shift), ("arrowleft", true, false));
        let p = key_press(&Key::Character("K".into()), ModifiersState::SHIFT).unwrap();
        assert_eq!(p.key, "k");
        assert!(key_press(&Key::Named(NamedKey::Shift), ModifiersState::SHIFT).is_none());
    }
}
