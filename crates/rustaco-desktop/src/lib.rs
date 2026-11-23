//! Native frontend for the Rustaco editor.
//!
//! [`DesktopEditor`] holds a document, its view, keymap and syntax highlighting (parsed on
//! a background thread), and draws each [`rustaco_view::Frame`] into a pixel buffer on the
//! CPU with glyphs from a system monospace font. A host feeds it window events (see
//! `examples/editor.rs` for a winit application) and presents the buffer, for example with
//! `softbuffer`. Rendering in software keeps the frontend small and portable; the frame is
//! the same one the browser draws.

mod font;
mod input;

use std::collections::HashMap;
use std::time::Duration;

pub use font::{Font, FontError};
pub use input::key_press;
use rustaco_core::{Document, KeyPress, Keymap, Platform, Resolved, Selection, Selections, default_keymap, word_at};
use rustaco_syntax::{BackgroundSyntax, Colors, Highlights, Theme, by_id, themes};
use rustaco_view::{Metrics, Plain, Rect, View};

/// Mouse buttons the editor reacts to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Middle,
    Right,
}

/// An editor surface: text, cursors, view and highlighting, drawn into a buffer.
pub struct DesktopEditor {
    pub doc: Document,
    pub view: View,
    keymap: Keymap,
    platform: Platform,
    font: Font,
    /// Physical pixels per logical pixel.
    scale: f32,
    theme: Theme,
    styles: Vec<u32>,
    colors: Colors,
    syntax: Option<BackgroundSyntax>,
    highlights: Option<Highlights>,
    /// What highlights were last asked for: model version and lines.
    asked: Option<(u64, std::ops::Range<usize>)>,
    clipboard: Option<arboard::Clipboard>,
    drag: Option<usize>,
    last_click: Option<(std::time::Instant, usize, u32)>,
    /// Chars of the IME preedit text in the document.
    preedit: usize,
    /// Set when something changed that needs a new frame.
    pub dirty: bool,
}

impl DesktopEditor {
    /// An editor for `text` in `language` (an id such as `"rust"`, or empty), drawn with
    /// `font` at `scale` physical pixels per logical pixel, in a `width` by `height` logical
    /// pixel area.
    pub fn new(text: &str, language: &str, font: Font, width: f32, height: f32, scale: f32) -> Self {
        let doc = Document::new(text);
        let metrics = Metrics::new(font.advance / scale, font.line_height / scale);
        let view = View::new(metrics, width, height);
        let platform = if cfg!(target_os = "macos") {
            Platform::Mac
        } else if cfg!(target_os = "linux") {
            Platform::Linux
        } else {
            Platform::Windows
        };
        let theme = themes::dark();
        let mut editor = Self {
            doc,
            view,
            keymap: default_keymap(platform),
            platform,
            font,
            scale,
            styles: Vec::new(),
            colors: theme.colors,
            theme,
            syntax: None,
            highlights: None,
            asked: None,
            clipboard: arboard::Clipboard::new().ok(),
            drag: None,
            last_click: None,
            preedit: 0,
            dirty: true,
        };
        editor.set_theme(themes::dark());
        editor.set_language(language);
        editor
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.styles = theme.style_table().iter().map(|s| s.foreground.unwrap_or(theme.colors.foreground)).collect();
        self.colors = theme.colors;
        self.theme = theme;
        self.dirty = true;
    }

    /// Highlights `language` on a background thread; empty or unknown ids turn it off.
    pub fn set_language(&mut self, language: &str) {
        let editing = rustaco_syntax::language_editing(language);
        self.doc.set_language_editing(editing);
        let model = self.doc.model();
        self.syntax = by_id(language).and_then(|def| BackgroundSyntax::spawn(def, model).ok());
        self.highlights = None;
        self.asked = None;
        self.dirty = true;
    }

    pub fn resize(&mut self, width: f32, height: f32, scale: f32) {
        if (scale - self.scale).abs() > f32::EPSILON {
            self.font = self.font.with_scale(scale / self.scale);
            self.scale = scale;
            self.view.metrics = Metrics::new(self.font.advance / scale, self.font.line_height / scale);
        }
        self.view.resize(width, height);
        self.dirty = true;
    }

    // ---- input ----

    /// A key went down. `text` is what it types, if anything (none with Ctrl or Cmd held).
    /// Returns whether the editor used it.
    pub fn key_down(&mut self, press: KeyPress, text: Option<&str>) -> bool {
        let clipboard_key = if self.platform == Platform::Mac { press.meta } else { press.ctrl };
        if clipboard_key && !press.alt && !press.shift {
            match press.key.as_str() {
                "c" => return self.copy(),
                "x" => return self.cut(),
                "v" => return self.paste(),
                _ => {}
            }
        }
        let context = HashMap::from([
            ("editorHasSelection".to_owned(), self.doc.selections().iter().any(|s| !s.is_empty())),
            ("editorHasMultipleSelections".to_owned(), self.doc.selections().len() > 1),
            ("editorTextFocus".to_owned(), true),
            ("inSnippetMode".to_owned(), self.doc.editor().snippet().is_some()),
        ]);
        match self.keymap.resolve(&press, &context) {
            Resolved::Command(command) => {
                self.run_command(&command);
                true
            }
            Resolved::Pending => true,
            Resolved::Unbound => match text {
                Some(t) if !t.is_empty() && !t.chars().any(char::is_control) => {
                    self.insert_text(t);
                    true
                }
                _ => false,
            },
        }
    }

    /// Runs a command by id; page moves and vertical moves go through the view so they step
    /// over folds and wrapped rows.
    pub fn run_command(&mut self, command: &str) -> bool {
        self.view.sync(self.doc.model());
        let page = self.view.page_rows() as isize;
        let vertical = match command {
            "cursorUp" => Some((-1, false)),
            "cursorUpSelect" => Some((-1, true)),
            "cursorDown" => Some((1, false)),
            "cursorDownSelect" => Some((1, true)),
            "cursorPageUp" => Some((-page, false)),
            "cursorPageUpSelect" => Some((-page, true)),
            "cursorPageDown" => Some((page, false)),
            "cursorPageDownSelect" => Some((page, true)),
            _ => None,
        };
        let handled = match vertical {
            Some((lines, extend)) => {
                let rope = self.doc.rope().clone();
                let targets: Vec<(usize, Option<usize>)> = self
                    .doc
                    .selections()
                    .iter()
                    .map(|s| self.view.vertical_target(&rope, s.head, s.goal_column, lines))
                    .collect();
                let mut it = targets.into_iter();
                self.doc.editor().move_heads(extend, |_, s| it.next().unwrap_or((s.head, s.goal_column)));
                true
            }
            None => self.doc.editor().run(command),
        };
        self.after_edit();
        handled
    }

    /// Types `text` at every cursor (one char goes through auto-closing pairs).
    pub fn insert_text(&mut self, text: &str) {
        let mut chars = text.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) => self.doc.editor().type_char(c),
            (Some(_), Some(_)) => self.doc.insert(text),
            _ => return,
        }
        self.after_edit();
    }

    /// IME composition text so far (empty when it was cancelled).
    pub fn ime_preedit(&mut self, text: &str) {
        let before = self.preedit;
        self.doc.editor().compose(before, text);
        self.preedit = text.chars().count();
        self.after_edit();
    }

    /// IME committed `text`, replacing the composition.
    pub fn ime_commit(&mut self, text: &str) {
        let before = self.preedit;
        self.doc.editor().compose(before, text);
        self.preedit = 0;
        self.after_edit();
    }

    fn after_edit(&mut self) {
        let head = self.doc.selections().primary().head;
        self.view.sync(self.doc.model());
        self.view.reveal(self.doc.rope(), head);
        self.dirty = true;
    }

    pub fn copy(&mut self) -> bool {
        let text = self.doc.editor().copy();
        self.clipboard.as_mut().is_some_and(|c| c.set_text(text).is_ok())
    }

    pub fn cut(&mut self) -> bool {
        let text = self.doc.editor().cut();
        let ok = self.clipboard.as_mut().is_some_and(|c| c.set_text(text).is_ok());
        self.after_edit();
        ok
    }

    pub fn paste(&mut self) -> bool {
        let Some(text) = self.clipboard.as_mut().and_then(|c| c.get_text().ok()) else { return false };
        self.doc.editor().paste(&text);
        self.after_edit();
        true
    }

    /// Mouse button down at a point in logical pixels. Double click selects a word, triple a
    /// line; Shift extends, Alt adds a cursor.
    pub fn mouse_down(&mut self, x: f32, y: f32, button: Button, shift: bool, alt: bool) {
        if button != Button::Left {
            return;
        }
        self.view.sync(self.doc.model());
        let pos = self.view.hit_test(self.doc.rope(), x, y);
        let now = std::time::Instant::now();
        let clicks = match self.last_click {
            Some((t, p, n)) if now.duration_since(t) < Duration::from_millis(400) && p == pos => n % 3 + 1,
            _ => 1,
        };
        self.last_click = Some((now, pos, clicks));
        let rope = self.doc.rope().clone();
        let selection = match clicks {
            2 => {
                let (a, b) = word_at(&rope, pos);
                Selection::new(a, b)
            }
            3 => {
                let line = rope.char_to_line(pos);
                Selection::new(rope.line_to_char(line), rope.line_to_char((line + 1).min(rope.len_lines())))
            }
            _ if shift => Selection::new(self.doc.selections().primary().anchor, pos),
            _ => Selection::cursor(pos),
        };
        if alt {
            let mut ranges = self.doc.selections().as_slice().to_vec();
            ranges.push(selection);
            let primary = ranges.len() - 1;
            self.doc.set_selections(Selections::new(ranges, primary));
        } else {
            self.doc.set_selections(Selections::single(selection));
        }
        self.drag = Some(self.doc.selections().primary().anchor);
        self.dirty = true;
    }

    pub fn mouse_move(&mut self, x: f32, y: f32) {
        let Some(anchor) = self.drag else { return };
        let pos = self.view.hit_test(self.doc.rope(), x, y);
        self.doc.set_selections(Selections::single(Selection::new(anchor, pos)));
        self.dirty = true;
    }

    pub fn mouse_up(&mut self) {
        self.drag = None;
    }

    /// Scrolls by logical pixels.
    pub fn wheel(&mut self, dx: f32, dy: f32) {
        self.view.sync(self.doc.model());
        let rope = self.doc.rope().clone();
        self.view.scroll_by(&rope, dx, dy as f64);
        self.dirty = true;
    }

    /// Where the primary caret is, in logical pixels: for placing the IME candidate window.
    pub fn caret_rect(&self) -> Option<Rect> {
        self.view.position_rect(self.doc.rope(), self.doc.selections().primary().head)
    }

    // ---- drawing ----

    /// Takes highlights that arrived and asks for those of the visible lines when the text
    /// or the view changed. Returns whether new highlights came in (draw again).
    pub fn poll_highlights(&mut self) -> bool {
        let Some(syntax) = self.syntax.as_mut() else { return false };
        let model = self.doc.model();
        let lines = self.view.visible_lines(model.rope());
        let key = (model.version(), lines.clone());
        if self.asked.as_ref() != Some(&key) {
            syntax.request_highlights(model, lines);
            self.asked = Some(key);
        }
        match syntax.poll(model) {
            Some(reply) => {
                self.highlights = Some(reply.highlights);
                if !reply.parsed {
                    // The tree still lags: ask again for final colours.
                    self.asked = None;
                }
                true
            }
            None => false,
        }
    }

    /// Draws the editor into `buffer` (0RGB pixels, `width` by `height` physical pixels).
    pub fn render(&mut self, buffer: &mut [u32], width: usize, height: usize) {
        self.dirty = false;
        let selections = self.doc.selections().clone();
        let frame = match &self.highlights {
            Some(h) => self.view.layout(self.doc.model(), &selections, h),
            None => self.view.layout(self.doc.model(), &selections, &Plain),
        };
        let mut canvas = Canvas { buffer, width, height, scale: self.scale };
        let c = &self.colors;
        canvas.fill(0.0, 0.0, width as f32, height as f32, c.background);
        if let Some(band) = frame.current_line {
            canvas.fill_rect(&band, c.current_line);
        }
        for r in &frame.selections {
            canvas.fill_rect(r, c.selection);
        }
        let ascent = self.font.ascent;
        for line in &frame.lines {
            let baseline =
                (line.y * self.scale + (line.height * self.scale - self.font.line_height) / 2.0 + ascent).round();
            for run in &line.runs {
                let color = self.styles.get(run.style as usize).copied().unwrap_or(c.foreground);
                let mut x = run.x * self.scale;
                for ch in run.text.chars() {
                    let cells = unicode_cells(ch);
                    canvas.glyph(&mut self.font, ch, x, baseline, color);
                    x += self.font.advance * cells as f32;
                }
            }
        }
        for r in &frame.carets {
            canvas.fill_rect(r, c.caret);
        }
        // Gutter.
        let gutter = frame.gutter_width;
        canvas.fill(0.0, 0.0, gutter * self.scale, height as f32, c.gutter);
        let primary_line = self.doc.rope().char_to_line(selections.primary().head);
        for line in frame.lines.iter().filter(|l| l.row == 0) {
            let number = (line.line + 1).to_string();
            let color = if line.line == primary_line { c.line_number_active } else { c.line_number };
            let baseline =
                (line.y * self.scale + (line.height * self.scale - self.font.line_height) / 2.0 + ascent).round();
            let right = (gutter - 3.5 * self.view.metrics.char_width) * self.scale;
            let mut x = right - number.len() as f32 * self.font.advance;
            for ch in number.chars() {
                canvas.glyph(&mut self.font, ch, x, baseline, color);
                x += self.font.advance;
            }
        }
        // Scrollbar thumb.
        let rope = self.doc.rope();
        let content = self.view.content_height(rope) as f32;
        let h = self.view.height;
        if content > h {
            let thumb = (h * h / content).max(20.0);
            let top = self.view.scroll_top() as f32 / (content - h) * (h - thumb);
            canvas.fill(
                (self.view.width - 10.0) * self.scale,
                top * self.scale,
                10.0 * self.scale,
                thumb * self.scale,
                c.scrollbar,
            );
        }
    }
}

/// Cells a char takes, as the view counts them.
fn unicode_cells(c: char) -> usize {
    unicode_width::UnicodeWidthChar::width(c).unwrap_or(1).max(1)
}

/// A pixel buffer with drawing in logical coordinates.
struct Canvas<'a> {
    buffer: &'a mut [u32],
    width: usize,
    height: usize,
    scale: f32,
}

impl Canvas<'_> {
    fn fill_rect(&mut self, r: &Rect, rgba: u32) {
        let s = self.scale;
        self.fill(r.x * s, r.y * s, r.width * s, r.height * s, rgba);
    }

    /// Fills physical pixels, blending by the colour's alpha.
    fn fill(&mut self, x: f32, y: f32, w: f32, h: f32, rgba: u32) {
        let x0 = x.max(0.0).round() as usize;
        let y0 = y.max(0.0).round() as usize;
        let x1 = ((x + w).round().max(0.0) as usize).min(self.width);
        let y1 = ((y + h).round().max(0.0) as usize).min(self.height);
        let alpha = rgba & 0xff;
        for row in y0..y1 {
            let line = &mut self.buffer[row * self.width..row * self.width + self.width];
            for px in &mut line[x0.min(x1)..x1] {
                *px = blend(*px, rgba >> 8, alpha);
            }
        }
    }

    /// Draws `ch` with its pen at physical (`x`, `baseline`).
    fn glyph(&mut self, font: &mut Font, ch: char, x: f32, baseline: f32, rgba: u32) {
        let g = font.glyph(ch);
        let left = x.round() as i32 + g.left;
        let top = baseline as i32 - g.top;
        let rgb = rgba >> 8;
        for gy in 0..g.height {
            let py = top + gy as i32;
            if py < 0 || py >= self.height as i32 {
                continue;
            }
            for gx in 0..g.width {
                let px = left + gx as i32;
                if px < 0 || px >= self.width as i32 {
                    continue;
                }
                let coverage = g.coverage[gy * g.width + gx] as u32;
                if coverage == 0 {
                    continue;
                }
                let i = py as usize * self.width + px as usize;
                self.buffer[i] = blend(self.buffer[i], rgb, coverage);
            }
        }
    }
}

/// `src` (0RGB) over `dst` with `alpha` out of 255.
fn blend(dst: u32, src: u32, alpha: u32) -> u32 {
    if alpha >= 255 {
        return src;
    }
    let mix = |shift: u32| {
        let d = (dst >> shift) & 0xff;
        let s = (src >> shift) & 0xff;
        ((s * alpha + d * (255 - alpha)) / 255) << shift
    };
    mix(16) | mix(8) | mix(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blending_mixes_channels() {
        assert_eq!(blend(0x000000, 0xff8040, 255), 0xff8040);
        assert_eq!(blend(0x000000, 0xff8040, 0), 0x000000);
        assert_eq!(blend(0x000000, 0xfe8040, 128), 0x7f4020);
    }
}
