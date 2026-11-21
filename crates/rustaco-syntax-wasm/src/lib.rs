//! A language module for the browser editor.
//!
//! Each build enables one language pack through cargo features and becomes its own WASM
//! file, loaded the first time a model uses one of its languages. The editor keeps the
//! text; this module keeps a copy, applies the editor's edits to it, reparses
//! incrementally and returns highlights for the lines the editor shows.
//!
//! Parsing runs in slices: [`Highlighter::parse`] stops after a time budget and resumes on
//! the next call, and [`Highlighter::highlight`] works from the last tree (shifted by the
//! edits since) until the new one is ready. Every call takes and returns plain data, so the
//! module can also run in a Web Worker behind `postMessage` without changing this interface.

use rustaco_core::{EditEvent, Point, Rope, Utf16Position};
use rustaco_syntax::{Syntax, by_id, languages};
use wasm_bindgen::prelude::*;

/// Numbers per edit in [`Highlighter::edit`]: start, old end and new end in bytes, then
/// row and byte column of the start, old end and new end.
pub const EDIT_STRIDE: usize = 9;

#[wasm_bindgen]
pub struct Highlighter {
    rope: Rope,
    syntax: Syntax,
}

#[wasm_bindgen]
impl Highlighter {
    #[wasm_bindgen(constructor)]
    pub fn new(language: &str, text: &str) -> Result<Highlighter, JsValue> {
        let def =
            by_id(language).ok_or_else(|| JsValue::from_str(&format!("language not in this module: {language}")))?;
        let syntax = Syntax::new(def).map_err(|e| JsValue::from_str(&e))?;
        Ok(Highlighter { rope: Rope::from_str(text), syntax })
    }

    /// Ids of the languages this module parses.
    pub fn languages() -> Vec<String> {
        languages().iter().map(|l| l.id.to_owned()).collect()
    }

    /// Replaces the text; the next [`Highlighter::parse`] starts from scratch.
    pub fn reset(&mut self, text: &str) {
        self.rope = Rope::from_str(text);
        self.syntax.reset();
    }

    /// Parses for at most `budget_ms` milliseconds. Returns true when the tree matches the
    /// text; false means call again (on a later frame) to continue.
    pub fn parse(&mut self, budget_ms: f64) -> bool {
        let deadline = js_sys::Date::now() + budget_ms;
        self.syntax.parse_with_budget(&self.rope, || js_sys::Date::now() < deadline)
    }

    /// Applies edits in order; they take effect in highlighting after the next
    /// [`Highlighter::parse`] completes. `numbers` holds [`EDIT_STRIDE`] values per
    /// edit; `text` is the inserted texts concatenated, each as long as its edit's
    /// `new_end - start` bytes.
    pub fn edit(&mut self, numbers: &[u32], text: &str) -> Result<(), JsValue> {
        let mut offset = 0;
        for e in numbers.chunks_exact(EDIT_STRIDE) {
            let [start_byte, old_end_byte, new_end_byte] = [e[0], e[1], e[2]].map(|n| n as usize);
            let len = self.rope.len_bytes();
            if start_byte > old_end_byte || old_end_byte > len || new_end_byte < start_byte {
                return Err(JsValue::from_str("edit outside the text; call reset"));
            }
            let inserted = text
                .get(offset..offset + (new_end_byte - start_byte))
                .ok_or_else(|| JsValue::from_str("edit text does not match its byte range"))?;
            offset += inserted.len();
            let start = self.rope.byte_to_char(start_byte);
            let old_end = self.rope.byte_to_char(old_end_byte);
            self.rope.remove(start..old_end);
            self.rope.insert(start, inserted);
            let point = |row: u32, column: u32| Point { row: row as usize, column: column as usize };
            self.syntax.apply_edit(&EditEvent {
                start,
                old_end,
                new_end: start + inserted.chars().count(),
                start_byte,
                old_end_byte,
                new_end_byte,
                start_point: point(e[3], e[4]),
                old_end_point: point(e[5], e[6]),
                new_end_point: point(e[7], e[8]),
                start_utf16: Utf16Position::default(),
                old_end_utf16: Utf16Position::default(),
                text: String::new(),
            });
        }
        Ok(())
    }

    /// Highlights lines `first..end`, as quads: line, start column, end column (chars),
    /// style id.
    pub fn highlight(&self, first: u32, end: u32) -> Vec<u32> {
        let highlights = self.syntax.highlight(&self.rope, first as usize..end as usize);
        let mut out = Vec::new();
        for line in first..end {
            for span in highlights.line(line as usize) {
                out.extend([line, span.start as u32, span.end as u32, span.style]);
            }
        }
        out
    }

    /// The module's copy of the text; for tests that check it stays in step with the editor.
    pub fn text(&self) -> String {
        self.rope.to_string()
    }
}
