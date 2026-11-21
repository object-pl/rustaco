# Monaco keybindings: status

Monaco's default editor keybindings (Windows and Linux keys; macOS uses Cmd where Ctrl is listed) compared with Rustaco's default keymap (`crates/rustaco-core/src/keymap.rs`). Missing entries name the roadmap phase that adds them.

| Keys | Command | Status |
|---|---|---|
| Arrows, Shift+arrows | cursor movement and selection | done |
| Ctrl+Left / Ctrl+Right (+Shift) | word movement | done |
| Home / End (+Shift) | line start (smart) / end | done |
| Ctrl+Home / Ctrl+End (+Shift) | document start / end | done |
| PageUp / PageDown (+Shift) | page movement | done |
| Backspace / Delete | delete, empty bracket pairs together | done |
| Ctrl+Backspace / Ctrl+Delete | delete word | done |
| Enter | new line with indentation, bracket-aware | done |
| Tab / Shift+Tab | indent / outdent | done |
| Ctrl+] / Ctrl+[ | indent / outdent lines | done |
| Ctrl+A | select all | done |
| Ctrl+Z / Ctrl+Y / Ctrl+Shift+Z | undo / redo | done |
| Ctrl+U | cursor undo | done |
| Ctrl+X / Ctrl+C / Ctrl+V | clipboard (browser events) | done |
| Alt+Up / Alt+Down | move lines | done |
| Shift+Alt+Up / Shift+Alt+Down | copy lines | done |
| Ctrl+Shift+K | delete lines | done |
| Ctrl+Enter / Ctrl+Shift+Enter | insert line below / above | done |
| Ctrl+/ | toggle line comment | done |
| Shift+Alt+A | toggle block comment | done |
| Ctrl+D | add next occurrence | done |
| Ctrl+Shift+L | select all occurrences | done |
| Ctrl+Alt+Up / Ctrl+Alt+Down | add cursor above / below | done |
| Alt+click | add cursor | done |
| Shift+Alt+drag | column selection | done |
| Escape | remove secondary cursors / clear selection | done |
| Shift+Alt+Right / Shift+Alt+Left | expand / shrink selection | done |
| F3 / Shift+F3 | next / previous match | done |
| Ctrl+K Ctrl+X | trim trailing whitespace | done |
| Ctrl+F / Ctrl+H | find / replace widget | phase 6 (API exists: `find`, `replace`, `replaceAll`) |
| Ctrl+G | go to line | phase 6 |
| Ctrl+Shift+\ | jump to matching bracket | done |
| Ctrl+Shift+[ / Ctrl+Shift+] | fold / unfold | done |
| Ctrl+K Ctrl+L | toggle fold | done |
| Ctrl+K Ctrl+0 / Ctrl+K Ctrl+J | fold all / unfold all | done |
| Ctrl+Space | trigger suggestions | phase 8 |
| Ctrl+Shift+Space | parameter hints | phase 8 |
| F12 / Alt+F12 / Shift+F12 | go to definition / peek / references | phases 8 and 9 |
| F2 | rename symbol | phase 8 |
| Ctrl+. | quick fix | phase 8 |
| Shift+Alt+F | format document | phases 8 and 9 |
| Ctrl+K Ctrl+C / Ctrl+K Ctrl+U | add / remove line comment | not planned separately; Ctrl+/ toggles |
| Ctrl+Shift+Alt+arrows | column selection by keyboard | not done |
| Alt+Z | toggle word wrap | phase 5 |
