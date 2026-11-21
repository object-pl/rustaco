//! Browser frontend: a WASM editor that draws on a canvas.
//!
//! Rust owns the text model, layout and drawing. The JS side (`web/rustaco.js`)
//! owns the DOM: it forwards keyboard, text input, mouse and clipboard events
//! and calls [`Editor::render`] once per animation frame when something changed.
//!
//! A [`Model`] can be shown by several [`Editor`]s; each keeps its own cursors
//! and scroll position.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use rustaco_core::{Cursors, Editor as Commands, EditorId, Rope, Selection, Selections, TextModel, word_at};
use rustaco_view::{Metrics, Plain, Rect, View};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

struct Theme {
    background: &'static str,
    gutter: &'static str,
    line_number: &'static str,
    line_number_active: &'static str,
    text: &'static str,
    selection: &'static str,
    current_line: &'static str,
    caret: &'static str,
    scrollbar: &'static str,
}

const DARK: Theme = Theme {
    background: "#1e1e1e",
    gutter: "#1e1e1e",
    line_number: "#6e7681",
    line_number_active: "#cccccc",
    text: "#d4d4d4",
    selection: "#264f78",
    current_line: "#2a2d2e",
    caret: "#aeafad",
    scrollbar: "rgba(121,121,121,0.4)",
};

thread_local! {
    static NEXT_EDITOR_ID: Cell<u32> = const { Cell::new(1) };
}

/// A text model that one or more editors can show.
#[wasm_bindgen]
pub struct Model {
    inner: Rc<RefCell<TextModel>>,
}

#[wasm_bindgen]
impl Model {
    #[wasm_bindgen(constructor)]
    pub fn new(text: &str) -> Model {
        Model { inner: Rc::new(RefCell::new(TextModel::new(text))) }
    }

    #[wasm_bindgen(js_name = getValue)]
    pub fn get_value(&self) -> String {
        self.inner.borrow().text()
    }

    /// Replaces the text and clears the undo history.
    #[wasm_bindgen(js_name = setValue)]
    pub fn set_value(&self, text: &str) {
        self.inner.borrow_mut().set_text(text);
    }

    /// Increases with every text change; the JS side uses it to fire change events.
    pub fn version(&self) -> f64 {
        self.inner.borrow().version() as f64
    }
}

#[derive(Clone, Copy)]
enum DragUnit {
    Char,
    Word,
    Line,
}

/// Mouse drag in progress: the unit first clicked stays selected while the pointer moves.
#[derive(Clone, Copy)]
struct Drag {
    start: usize,
    end: usize,
    unit: DragUnit,
    /// Alt-drag edits only the newly added selection.
    add: bool,
}

#[wasm_bindgen]
pub struct Editor {
    model: Rc<RefCell<TextModel>>,
    cursors: Cursors,
    view: View,
    canvas: HtmlCanvasElement,
    ctx: CanvasRenderingContext2d,
    font: String,
    dpr: f64,
    drag: Option<Drag>,
    caret: Rect,
}

#[wasm_bindgen]
impl Editor {
    #[wasm_bindgen(constructor)]
    pub fn new(canvas: HtmlCanvasElement, model: &Model, font_family: &str, font_size: f64) -> Result<Editor, JsValue> {
        let ctx: CanvasRenderingContext2d =
            canvas.get_context("2d")?.ok_or_else(|| JsValue::from_str("canvas has no 2d context"))?.dyn_into()?;
        let font = format!("{font_size}px {font_family}");
        ctx.set_font(&font);
        let sample = "M".repeat(64);
        let char_width = (ctx.measure_text(&sample)?.width() / 64.0) as f32;
        let metrics = Metrics { char_width, line_height: (font_size * 1.5).round() as f32 };
        let id = EditorId(NEXT_EDITOR_ID.with(|n| n.replace(n.get() + 1)));
        let cursors = Cursors::new(id, &model.inner.borrow());
        Ok(Editor {
            model: model.inner.clone(),
            cursors,
            view: View::new(metrics, canvas.client_width() as f32, canvas.client_height() as f32),
            canvas,
            ctx,
            font,
            dpr: 1.0,
            drag: None,
            caret: Rect::default(),
        })
    }

    pub fn resize(&mut self, width: f64, height: f64, dpr: f64) {
        self.dpr = dpr;
        self.canvas.set_width((width * dpr).round() as u32);
        self.canvas.set_height((height * dpr).round() as u32);
        self.view.resize(width as f32, height as f32);
        let (top, left) = (self.view.scroll_top(), self.view.scroll_left());
        self.view.scroll_to(self.model.borrow().rope(), top, left);
    }

    /// Selections as a flat list of char offsets: anchor, head, anchor, head, ...
    /// The primary selection comes first.
    pub fn selections(&mut self) -> Vec<u32> {
        self.cursors.sync(&self.model.borrow());
        let sels = self.cursors.selections();
        let primary = sels.primary_index();
        let order = std::iter::once(primary).chain((0..sels.len()).filter(|&i| i != primary));
        order.flat_map(|i| [sels.as_slice()[i].anchor as u32, sels.as_slice()[i].head as u32]).collect()
    }

    /// Replaces the selections; takes the same flat layout [`Editor::selections`] returns.
    pub fn set_selections(&mut self, flat: &[u32]) {
        let ranges: Vec<Selection> =
            flat.chunks_exact(2).map(|p| Selection::new(p[0] as usize, p[1] as usize)).collect();
        if ranges.is_empty() {
            return;
        }
        self.edit(|ed| ed.set_selections(Selections::new(ranges, 0)));
        self.reveal_primary();
    }

    /// Scrolls to the top; used after the model's text is replaced.
    pub fn scroll_to_top(&mut self) {
        self.view.scroll_to(self.model.borrow().rope(), 0.0, 0.0);
    }

    pub fn line_height(&self) -> f64 {
        self.view.metrics.line_height as f64
    }

    /// Primary caret position in CSS pixels, for placing the hidden textarea (IME popups).
    pub fn caret_x(&self) -> f64 {
        self.caret.x as f64
    }

    pub fn caret_y(&self) -> f64 {
        self.caret.y as f64
    }

    /// Handles a keydown. Returns true when the key was consumed and the default action
    /// must be prevented. Printable text arrives separately through [`Editor::insert_text`].
    pub fn key_down(&mut self, key: &str, ctrl: bool, shift: bool, alt: bool, meta: bool) -> bool {
        let m = ctrl || meta;
        let page = ((self.view.height / self.view.metrics.line_height) as isize - 1).max(1);
        let mut scroll_lines = 0isize;
        let handled = self.edit(|d| {
            match key {
                "ArrowLeft" if m => d.move_word_left(shift),
                "ArrowLeft" => d.move_left(shift),
                "ArrowRight" if m => d.move_word_right(shift),
                "ArrowRight" => d.move_right(shift),
                "ArrowUp" if m && alt => d.add_cursor_vertical(false),
                "ArrowUp" => d.move_up(shift),
                "ArrowDown" if m && alt => d.add_cursor_vertical(true),
                "ArrowDown" => d.move_down(shift),
                "Home" if m => d.move_document_start(shift),
                "Home" => d.move_line_start(shift),
                "End" if m => d.move_document_end(shift),
                "End" => d.move_line_end(shift),
                "PageUp" => {
                    d.move_vertical(-page, shift);
                    scroll_lines = -page;
                }
                "PageDown" => {
                    d.move_vertical(page, shift);
                    scroll_lines = page;
                }
                "Backspace" if m => d.delete_word_backward(),
                "Backspace" => d.delete_backward(),
                "Delete" if m => d.delete_word_forward(),
                "Delete" => d.delete_forward(),
                "Enter" => d.newline(),
                "Tab" => d.indent(),
                "Escape" => {
                    let primary = d.selections().primary();
                    let collapsed = if d.selections().len() > 1 { primary } else { Selection::cursor(primary.head) };
                    d.set_selections(Selections::single(collapsed));
                }
                _ if m && !alt => match key.to_ascii_lowercase().as_str() {
                    "a" => d.select_all(),
                    "z" if shift => {
                        d.redo();
                    }
                    "z" => {
                        d.undo();
                    }
                    "y" => {
                        d.redo();
                    }
                    _ => return false,
                },
                _ => return false,
            }
            true
        });
        if !handled {
            return false;
        }
        if scroll_lines != 0 {
            let dy = scroll_lines as f32 * self.view.metrics.line_height;
            self.view.scroll_by(self.model.borrow().rope(), 0.0, dy);
        }
        self.reveal_primary();
        true
    }

    pub fn insert_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.edit(|ed| ed.insert(text));
        self.reveal_primary();
    }

    pub fn copy(&mut self) -> String {
        self.edit(|ed| ed.copy())
    }

    pub fn cut(&mut self) -> String {
        let text = self.edit(|ed| ed.cut());
        self.reveal_primary();
        text
    }

    pub fn paste(&mut self, text: &str) {
        self.edit(|ed| ed.paste(text));
        self.reveal_primary();
    }

    pub fn wheel(&mut self, dx: f64, dy: f64) {
        self.view.scroll_by(self.model.borrow().rope(), dx as f32, dy as f32);
    }

    /// `clicks` is the DOM `detail`: 1 places a cursor, 2 selects a word, 3 a line.
    pub fn mouse_down(&mut self, x: f64, y: f64, clicks: u32, shift: bool, alt: bool) {
        let unit = match clicks {
            0 | 1 => DragUnit::Char,
            2 => DragUnit::Word,
            _ => DragUnit::Line,
        };
        let (pos, (start, end)) = {
            let model = self.model.borrow();
            let pos = self.view.hit_test(model.rope(), x as f32, y as f32);
            (pos, unit_range(model.rope(), unit, pos))
        };
        self.drag = Some(self.edit(|ed| {
            let sels = ed.selections();
            if shift && clicks <= 1 {
                let anchor = sels.primary().anchor;
                ed.set_selections(Selections::single(Selection::new(anchor, pos)));
                Drag { start: anchor, end: anchor, unit, add: false }
            } else if alt {
                let mut ranges = sels.as_slice().to_vec();
                ranges.push(Selection::new(start, end));
                let primary = ranges.len() - 1;
                ed.set_selections(Selections::new(ranges, primary));
                Drag { start, end, unit, add: true }
            } else {
                ed.set_selections(Selections::single(Selection::new(start, end)));
                Drag { start, end, unit, add: false }
            }
        }));
    }

    /// Extends the selection while a button is held. Returns false when no drag is active.
    pub fn mouse_move(&mut self, x: f64, y: f64) -> bool {
        let Some(drag) = self.drag else { return false };
        let (start, end) = {
            let model = self.model.borrow();
            let pos = self.view.hit_test(model.rope(), x as f32, y as f32);
            unit_range(model.rope(), drag.unit, pos)
        };
        let sel = if start < drag.start { Selection::new(drag.end, start) } else { Selection::new(drag.start, end) };
        self.edit(|ed| {
            let sels = ed.selections();
            let next = if drag.add {
                let mut ranges = sels.as_slice().to_vec();
                ranges[sels.primary_index()] = sel;
                Selections::new(ranges, sels.primary_index())
            } else {
                Selections::single(sel)
            };
            ed.set_selections(next);
        });
        self.reveal_primary();
        true
    }

    pub fn mouse_up(&mut self) {
        self.drag = None;
    }

    pub fn render(&mut self, caret_visible: bool) -> Result<(), JsValue> {
        let model = self.model.borrow();
        self.cursors.sync(&model);
        let selections = self.cursors.selections();
        let frame = self.view.layout(&model, selections, &Plain);
        let ctx = &self.ctx;
        let (w, h) = (self.view.width as f64, self.view.height as f64);
        let gutter = frame.gutter_width as f64;
        let cw = self.view.metrics.char_width as f64;
        let fill = |color: &str, r: &Rect| {
            ctx.set_fill_style_str(color);
            ctx.fill_rect(r.x as f64, r.y as f64, r.width as f64, r.height as f64);
        };

        ctx.set_transform(self.dpr, 0.0, 0.0, self.dpr, 0.0, 0.0)?;
        ctx.set_fill_style_str(DARK.background);
        ctx.fill_rect(0.0, 0.0, w, h);
        if let Some(band) = &frame.current_line {
            fill(DARK.current_line, band);
        }

        ctx.save();
        ctx.begin_path();
        ctx.rect(gutter, 0.0, w - gutter, h);
        ctx.clip();
        for r in &frame.selections {
            fill(DARK.selection, r);
        }
        ctx.set_font(&self.font);
        ctx.set_text_baseline("middle");
        ctx.set_text_align("left");
        ctx.set_fill_style_str(DARK.text);
        for line in &frame.lines {
            let y = (line.y + line.height / 2.0) as f64;
            for run in &line.runs {
                ctx.fill_text(&run.text, run.x as f64, y)?;
            }
        }
        if caret_visible {
            for r in &frame.carets {
                fill(DARK.caret, r);
            }
        }
        ctx.restore();

        ctx.set_fill_style_str(DARK.gutter);
        ctx.fill_rect(0.0, 0.0, gutter, h);
        ctx.set_text_align("right");
        let rope = model.rope();
        let primary_line = rope.char_to_line(selections.primary().head);
        for line in &frame.lines {
            let color = if line.line == primary_line { DARK.line_number_active } else { DARK.line_number };
            ctx.set_fill_style_str(color);
            let y = (line.y + line.height / 2.0) as f64;
            ctx.fill_text(&(line.line + 1).to_string(), gutter - 1.5 * cw, y)?;
        }

        let content = self.view.content_height(rope) as f64;
        if content > h {
            let thumb = (h * h / content).max(20.0);
            let top = self.view.scroll_top() as f64 / (content - h) * (h - thumb);
            ctx.set_fill_style_str(DARK.scrollbar);
            ctx.fill_rect(w - 10.0, top, 10.0, thumb);
        }

        // Carets are laid out in selection order, but off-screen ones are skipped.
        let visible = self.view.visible_lines(rope);
        let primary_caret = selections
            .iter()
            .take(selections.primary_index())
            .filter(|s| visible.contains(&rope.char_to_line(s.head)))
            .count()
            .min(frame.carets.len().saturating_sub(1));
        if let Some(c) = frame.carets.get(primary_caret) {
            self.caret = *c;
        }
        Ok(())
    }
}

impl Editor {
    /// Runs editing commands for this editor against the shared model.
    fn edit<R>(&mut self, f: impl FnOnce(&mut Commands) -> R) -> R {
        let mut model = self.model.borrow_mut();
        f(&mut Commands::new(&mut model, &mut self.cursors))
    }

    fn reveal_primary(&mut self) {
        let model = self.model.borrow();
        self.cursors.sync(&model);
        let head = self.cursors.selections().primary().head;
        self.view.reveal(model.rope(), head);
    }
}

fn unit_range(rope: &Rope, unit: DragUnit, pos: usize) -> (usize, usize) {
    match unit {
        DragUnit::Char => (pos, pos),
        DragUnit::Word => word_at(rope, pos),
        DragUnit::Line => {
            let line = rope.char_to_line(pos);
            (rope.line_to_char(line), rope.line_to_char(line + 1))
        }
    }
}
