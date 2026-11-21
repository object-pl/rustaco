//! Keybindings: key chords mapped to command ids, with `when` conditions and chords of two
//! keys. Keymaps are data (VS Code's format, flattened), so presets and user overrides are
//! lists of entries, not code.

use std::collections::HashMap;

/// A key with modifiers. `key` is the `KeyboardEvent.key` value, lowercased (`"a"`, `"/"`,
/// `"arrowup"`, `"enter"`, `"f2"`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct KeyPress {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
    pub key: String,
}

impl KeyPress {
    /// Parses `"ctrl+shift+k"`, `"cmd+/"`, `"alt+up"`. `cmd` and `win` mean the meta key;
    /// `up`, `down`, `left`, `right` and `esc` are short for the DOM key names.
    pub fn parse(s: &str) -> Option<KeyPress> {
        let mut k = KeyPress { ctrl: false, shift: false, alt: false, meta: false, key: String::new() };
        let lower = s.to_ascii_lowercase();
        let mut parts: Vec<&str> = lower.split('+').collect();
        // "ctrl++" binds the plus key.
        if lower.ends_with("++") {
            parts.retain(|p| !p.is_empty());
            parts.push("+");
        }
        let (last, mods) = parts.split_last()?;
        for m in mods {
            match *m {
                "ctrl" => k.ctrl = true,
                "shift" => k.shift = true,
                "alt" | "option" => k.alt = true,
                "cmd" | "meta" | "win" | "super" => k.meta = true,
                _ => return None,
            }
        }
        k.key = match *last {
            "up" => "arrowup",
            "down" => "arrowdown",
            "left" => "arrowleft",
            "right" => "arrowright",
            "esc" => "escape",
            "space" => " ",
            "pageup" => "pageup",
            "pagedown" => "pagedown",
            other => other,
        }
        .to_owned();
        (!k.key.is_empty()).then_some(k)
    }

    /// Whether a pressed key matches this binding key. For symbol keys (not letters, digits
    /// or named keys) shift is ignored: on many layouts `/` or `[` need shift to type, and a
    /// `ctrl+/` binding should still fire.
    fn matches(&self, pressed: &KeyPress) -> bool {
        let symbol = self.key.chars().count() == 1 && !self.key.chars().all(char::is_alphanumeric);
        self.key == pressed.key
            && self.ctrl == pressed.ctrl
            && self.alt == pressed.alt
            && self.meta == pressed.meta
            && (symbol || self.shift == pressed.shift)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    /// One key, or two for a chord (`ctrl+k ctrl+x`).
    pub keys: Vec<KeyPress>,
    pub command: String,
    pub when: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Windows,
    Linux,
    Mac,
}

/// What a key press resolved to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolved {
    Command(String),
    /// The first key of a chord; the next key completes it.
    Pending,
    /// Not bound: the frontend lets the key through (typing, AltGr characters).
    Unbound,
}

#[derive(Clone, Debug, Default)]
pub struct Keymap {
    bindings: Vec<Binding>,
    pending: Option<KeyPress>,
}

impl Keymap {
    /// Builds a keymap from (keys, command, when) entries. Later entries win over earlier
    /// ones for the same keys, so user entries go last. A command starting with `-` removes
    /// that command's earlier binding for the keys, as in VS Code.
    pub fn from_entries<'a>(entries: impl IntoIterator<Item = (&'a str, &'a str, Option<&'a str>)>) -> Keymap {
        let mut bindings: Vec<Binding> = Vec::new();
        for (keys, command, when) in entries {
            let Some(keys) = keys.split_whitespace().map(KeyPress::parse).collect::<Option<Vec<_>>>() else { continue };
            if let Some(removed) = command.strip_prefix('-') {
                bindings.retain(|b| !(b.keys == keys && b.command == removed));
                continue;
            }
            bindings.push(Binding { keys, command: command.to_owned(), when: when.map(str::to_owned) });
        }
        Keymap { bindings, pending: None }
    }

    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }

    /// Appends entries (user overrides) on top of the current bindings.
    pub fn extend<'a>(&mut self, entries: impl IntoIterator<Item = (&'a str, &'a str, Option<&'a str>)>) {
        let mut all: Vec<(String, String, Option<String>)> =
            self.bindings.iter().map(|b| (keys_to_string(&b.keys), b.command.clone(), b.when.clone())).collect();
        all.extend(entries.into_iter().map(|(k, c, w)| (k.to_owned(), c.to_owned(), w.map(str::to_owned))));
        *self = Keymap::from_entries(all.iter().map(|(k, c, w)| (k.as_str(), c.as_str(), w.as_deref())));
    }

    /// Resolves a key press against the bindings whose `when` holds in `context`. The last
    /// matching binding wins.
    pub fn resolve(&mut self, pressed: &KeyPress, context: &HashMap<String, bool>) -> Resolved {
        // Pressing a modifier on its own (the first event of every shortcut) neither matches
        // nor cancels a pending chord.
        if matches!(pressed.key.as_str(), "control" | "shift" | "alt" | "meta" | "os" | "altgraph" | "capslock") {
            return Resolved::Unbound;
        }
        let applies = |b: &Binding| b.when.as_deref().is_none_or(|w| eval_when(w, context));
        if let Some(first) = self.pending.take() {
            return self
                .bindings
                .iter()
                .rev()
                .find(|b| b.keys.len() == 2 && b.keys[0].matches(&first) && b.keys[1].matches(pressed) && applies(b))
                .map_or(Resolved::Unbound, |b| Resolved::Command(b.command.clone()));
        }
        if let Some(b) =
            self.bindings.iter().rev().find(|b| b.keys.len() == 1 && b.keys[0].matches(pressed) && applies(b))
        {
            return Resolved::Command(b.command.clone());
        }
        if self.bindings.iter().any(|b| b.keys.len() == 2 && b.keys[0].matches(pressed) && applies(b)) {
            self.pending = Some(pressed.clone());
            return Resolved::Pending;
        }
        Resolved::Unbound
    }

    pub fn has_pending_chord(&self) -> bool {
        self.pending.is_some()
    }
}

fn keys_to_string(keys: &[KeyPress]) -> String {
    keys.iter()
        .map(|k| {
            let mut s = String::new();
            for (on, name) in [(k.ctrl, "ctrl+"), (k.shift, "shift+"), (k.alt, "alt+"), (k.meta, "cmd+")] {
                if on {
                    s.push_str(name);
                }
            }
            s.push_str(if k.key == " " { "space" } else { &k.key });
            s
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Evaluates a `when` clause: context keys combined with `!`, `&&` and `||` (`&&` binds
/// tighter). Unknown keys are false.
pub fn eval_when(expr: &str, context: &HashMap<String, bool>) -> bool {
    expr.split("||").any(|conj| {
        conj.split("&&").all(|term| {
            let term = term.trim();
            match term.strip_prefix('!') {
                Some(key) => !context.get(key.trim()).copied().unwrap_or(false),
                None => context.get(term).copied().unwrap_or(false),
            }
        })
    })
}

/// (keys, command, when) for one platform.
type Entry = (&'static str, &'static str, Option<&'static str>);

const SHARED: &[Entry] = &[
    ("left", "cursorLeft", None),
    ("shift+left", "cursorLeftSelect", None),
    ("right", "cursorRight", None),
    ("shift+right", "cursorRightSelect", None),
    ("up", "cursorUp", None),
    ("shift+up", "cursorUpSelect", None),
    ("down", "cursorDown", None),
    ("shift+down", "cursorDownSelect", None),
    ("pageup", "cursorPageUp", None),
    ("shift+pageup", "cursorPageUpSelect", None),
    ("pagedown", "cursorPageDown", None),
    ("shift+pagedown", "cursorPageDownSelect", None),
    ("backspace", "deleteLeft", None),
    ("shift+backspace", "deleteLeft", None),
    ("delete", "deleteRight", None),
    ("enter", "editor.action.newline", None),
    ("shift+enter", "editor.action.newline", None),
    ("tab", "tab", None),
    ("shift+tab", "outdent", None),
    ("escape", "removeSecondaryCursors", Some("editorHasMultipleSelections")),
    ("escape", "cancelSelection", Some("editorHasSelection && !editorHasMultipleSelections")),
    ("alt+up", "editor.action.moveLinesUpAction", None),
    ("alt+down", "editor.action.moveLinesDownAction", None),
    ("shift+alt+up", "editor.action.copyLinesUpAction", None),
    ("shift+alt+down", "editor.action.copyLinesDownAction", None),
    ("shift+alt+a", "editor.action.blockComment", None),
];

const WINDOWS_LINUX: &[Entry] = &[
    ("ctrl+left", "cursorWordLeft", None),
    ("ctrl+shift+left", "cursorWordLeftSelect", None),
    ("ctrl+right", "cursorWordRight", None),
    ("ctrl+shift+right", "cursorWordRightSelect", None),
    ("home", "cursorHome", None),
    ("shift+home", "cursorHomeSelect", None),
    ("end", "cursorEnd", None),
    ("shift+end", "cursorEndSelect", None),
    ("ctrl+home", "cursorTop", None),
    ("ctrl+shift+home", "cursorTopSelect", None),
    ("ctrl+end", "cursorBottom", None),
    ("ctrl+shift+end", "cursorBottomSelect", None),
    ("ctrl+backspace", "deleteWordLeft", None),
    ("ctrl+delete", "deleteWordRight", None),
    ("ctrl+a", "editor.action.selectAll", None),
    ("ctrl+z", "undo", None),
    ("ctrl+y", "redo", None),
    ("ctrl+shift+z", "redo", None),
    ("ctrl+enter", "editor.action.insertLineAfter", None),
    ("ctrl+shift+enter", "editor.action.insertLineBefore", None),
    ("ctrl+shift+k", "editor.action.deleteLines", None),
    ("ctrl+/", "editor.action.commentLine", None),
    ("ctrl+d", "editor.action.addSelectionToNextFindMatch", None),
    ("ctrl+shift+l", "editor.action.selectHighlights", None),
    ("ctrl+alt+up", "editor.action.insertCursorAbove", None),
    ("ctrl+alt+down", "editor.action.insertCursorBelow", None),
    ("ctrl+k ctrl+x", "editor.action.trimTrailingWhitespace", None),
    ("ctrl+]", "editor.action.indentLines", None),
    ("ctrl+[", "editor.action.outdentLines", None),
];

const MAC: &[Entry] = &[
    ("alt+left", "cursorWordLeft", None),
    ("shift+alt+left", "cursorWordLeftSelect", None),
    ("alt+right", "cursorWordRight", None),
    ("shift+alt+right", "cursorWordRightSelect", None),
    ("cmd+left", "cursorHome", None),
    ("shift+cmd+left", "cursorHomeSelect", None),
    ("cmd+right", "cursorEnd", None),
    ("shift+cmd+right", "cursorEndSelect", None),
    ("home", "cursorHome", None),
    ("end", "cursorEnd", None),
    ("cmd+up", "cursorTop", None),
    ("shift+cmd+up", "cursorTopSelect", None),
    ("cmd+down", "cursorBottom", None),
    ("shift+cmd+down", "cursorBottomSelect", None),
    ("alt+backspace", "deleteWordLeft", None),
    ("alt+delete", "deleteWordRight", None),
    ("cmd+a", "editor.action.selectAll", None),
    ("cmd+z", "undo", None),
    ("shift+cmd+z", "redo", None),
    ("cmd+enter", "editor.action.insertLineAfter", None),
    ("shift+cmd+enter", "editor.action.insertLineBefore", None),
    ("shift+cmd+k", "editor.action.deleteLines", None),
    ("cmd+/", "editor.action.commentLine", None),
    ("cmd+d", "editor.action.addSelectionToNextFindMatch", None),
    ("shift+cmd+l", "editor.action.selectHighlights", None),
    ("alt+cmd+up", "editor.action.insertCursorAbove", None),
    ("alt+cmd+down", "editor.action.insertCursorBelow", None),
    ("cmd+k cmd+x", "editor.action.trimTrailingWhitespace", None),
    ("cmd+]", "editor.action.indentLines", None),
    ("cmd+[", "editor.action.outdentLines", None),
];

/// JetBrains-style bindings, layered over the default keymap.
const JETBRAINS: &[Entry] = &[
    ("ctrl+d", "editor.action.copyLinesDownAction", None),
    ("ctrl+y", "editor.action.deleteLines", None),
    ("ctrl+shift+up", "editor.action.moveLinesUpAction", None),
    ("ctrl+shift+down", "editor.action.moveLinesDownAction", None),
    ("alt+j", "editor.action.addSelectionToNextFindMatch", None),
    ("ctrl+shift+alt+j", "editor.action.selectHighlights", None),
    ("ctrl+shift+u", "editor.action.transformToUppercase", None),
];

/// The default keymap, following VS Code and Monaco.
pub fn default_keymap(platform: Platform) -> Keymap {
    let specific = if platform == Platform::Mac { MAC } else { WINDOWS_LINUX };
    Keymap::from_entries(SHARED.iter().chain(specific).copied())
}

/// Bindings modelled on JetBrains IDEs over the default keymap (Windows and Linux keys).
pub fn jetbrains_keymap(platform: Platform) -> Keymap {
    let specific = if platform == Platform::Mac { MAC } else { WINDOWS_LINUX };
    Keymap::from_entries(SHARED.iter().chain(specific).chain(JETBRAINS).copied())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(s: &str) -> KeyPress {
        KeyPress::parse(s).unwrap()
    }

    fn ctx(pairs: &[(&str, bool)]) -> HashMap<String, bool> {
        pairs.iter().map(|&(k, v)| (k.to_owned(), v)).collect()
    }

    #[test]
    fn parses_keys() {
        assert_eq!(key("Ctrl+Shift+K"), KeyPress { ctrl: true, shift: true, alt: false, meta: false, key: "k".into() });
        assert_eq!(key("cmd+up").key, "arrowup");
        assert_eq!(key("ctrl++").key, "+");
        assert!(KeyPress::parse("hyper+x").is_none());
    }

    #[test]
    fn resolves_single_keys_chords_and_when() {
        let mut km = default_keymap(Platform::Windows);
        let none = ctx(&[]);
        assert_eq!(km.resolve(&key("ctrl+/"), &none), Resolved::Command("editor.action.commentLine".into()));
        // Shift is ignored for symbol keys: on a German layout `/` is shift+7.
        assert_eq!(km.resolve(&key("ctrl+shift+/"), &none), Resolved::Command("editor.action.commentLine".into()));
        assert_eq!(km.resolve(&key("ctrl+k"), &none), Resolved::Pending);
        // The keydown of Ctrl alone, before the X, keeps the chord pending.
        assert_eq!(km.resolve(&key("ctrl+control"), &none), Resolved::Unbound);
        assert_eq!(km.resolve(&key("ctrl+x"), &none), Resolved::Command("editor.action.trimTrailingWhitespace".into()));
        assert_eq!(km.resolve(&key("escape"), &none), Resolved::Unbound);
        assert_eq!(
            km.resolve(&key("escape"), &ctx(&[("editorHasMultipleSelections", true), ("editorHasSelection", true)])),
            Resolved::Command("removeSecondaryCursors".into())
        );
        // AltGr arrives as ctrl+alt: unbound letters stay free for typing (Polish ą, ę).
        assert_eq!(km.resolve(&key("ctrl+alt+a"), &none), Resolved::Unbound);
    }

    #[test]
    fn later_entries_override_and_minus_removes() {
        let mut km = default_keymap(Platform::Windows);
        km.extend([("ctrl+d", "editor.action.copyLinesDownAction", None)]);
        assert_eq!(
            km.resolve(&key("ctrl+d"), &ctx(&[])),
            Resolved::Command("editor.action.copyLinesDownAction".into())
        );
        km.extend([("ctrl+d", "-editor.action.copyLinesDownAction", None)]);
        assert_eq!(
            km.resolve(&key("ctrl+d"), &ctx(&[])),
            Resolved::Command("editor.action.addSelectionToNextFindMatch".into())
        );
        let mut jb = jetbrains_keymap(Platform::Windows);
        assert_eq!(jb.resolve(&key("ctrl+y"), &ctx(&[])), Resolved::Command("editor.action.deleteLines".into()));
    }

    #[test]
    fn when_clauses() {
        let c = ctx(&[("a", true), ("b", false)]);
        assert!(eval_when("a && !b", &c));
        assert!(eval_when("b || a", &c));
        assert!(!eval_when("a && b", &c));
        assert!(!eval_when("unknown", &c));
    }
}
