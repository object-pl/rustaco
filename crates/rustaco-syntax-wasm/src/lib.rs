//! A language module for the browser editor.
//!
//! Each build enables one language pack through cargo features and becomes its own WASM
//! file, loaded the first time a model uses one of its languages. The editor keeps the
//! text; this module keeps a copy, applies the editor's edits to it, reparses
//! incrementally and returns highlights for the lines the editor shows. A pack uses either
//! tree-sitter grammars or, for the `textmate` pack, TextMate grammars; the interface is the
//! same.
//!
//! Parsing runs in slices: [`Highlighter::parse`] stops after a time budget and resumes on
//! the next call, and [`Highlighter::highlight`] works from the last result (shifted by the
//! edits since) until the new one is ready. Every call takes and returns plain data, so the
//! module can also run in a Web Worker behind `postMessage` without changing this interface.

use rustaco_core::{EditEvent, Point, Rope, Utf16Position};
// Used only through cfg-gated paths; keeps a featureless build (the workspace check) warning-free.
use rustaco_syntax as _;
use wasm_bindgen::prelude::*;

/// Numbers per edit in [`Highlighter::edit`]: start, old end and new end in bytes, then
/// row and byte column of the start, old end and new end.
pub const EDIT_STRIDE: usize = 9;

enum Engine {
    #[cfg(feature = "tree-sitter")]
    TreeSitter(rustaco_syntax::Syntax),
    #[cfg(feature = "textmate")]
    TextMate(rustaco_syntax::TextMateSyntax),
    /// Only in builds with no language enabled (the workspace check); never constructed.
    #[allow(dead_code)]
    Unavailable,
}

impl Engine {
    fn new(language: &str) -> Result<Engine, String> {
        #[cfg(feature = "tree-sitter")]
        if let Some(def) = rustaco_syntax::by_id(language) {
            return rustaco_syntax::Syntax::new(def).map(Engine::TreeSitter);
        }
        #[cfg(feature = "textmate")]
        if let Some(grammar) = rustaco_syntax::textmate_grammar(language) {
            return Ok(Engine::TextMate(rustaco_syntax::TextMateSyntax::new(grammar)));
        }
        Err(format!("language not in this module: {language}"))
    }
}

#[wasm_bindgen]
pub struct Highlighter {
    rope: Rope,
    engine: Engine,
}

#[wasm_bindgen]
impl Highlighter {
    #[wasm_bindgen(constructor)]
    pub fn new(language: &str, text: &str) -> Result<Highlighter, JsValue> {
        let engine = Engine::new(language).map_err(|e| JsValue::from_str(&e))?;
        Ok(Highlighter { rope: Rope::from_str(text), engine })
    }

    /// Ids of the languages this module parses.
    pub fn languages() -> Vec<String> {
        #[allow(unused_mut)]
        let mut ids: Vec<String> = Vec::new();
        #[cfg(feature = "tree-sitter")]
        ids.extend(rustaco_syntax::languages().iter().map(|l| l.id.to_owned()));
        #[cfg(feature = "textmate")]
        ids.extend(rustaco_syntax::textmate_languages().map(str::to_owned));
        ids
    }

    /// Replaces the text; the next [`Highlighter::parse`] starts from scratch.
    pub fn reset(&mut self, text: &str) {
        self.rope = Rope::from_str(text);
        match &mut self.engine {
            #[cfg(feature = "tree-sitter")]
            Engine::TreeSitter(s) => s.reset(),
            #[cfg(feature = "textmate")]
            Engine::TextMate(s) => s.reset(),
            Engine::Unavailable => {}
        }
    }

    /// Parses for at most `budget_ms` milliseconds. Returns true when the result matches the
    /// text; false means call again (on a later frame) to continue.
    pub fn parse(&mut self, budget_ms: f64) -> bool {
        let deadline = js_sys::Date::now() + budget_ms;
        let keep_going = || js_sys::Date::now() < deadline;
        match &mut self.engine {
            #[cfg(feature = "tree-sitter")]
            Engine::TreeSitter(s) => s.parse_with_budget(&self.rope, keep_going),
            #[cfg(feature = "textmate")]
            Engine::TextMate(s) => s.parse_with_budget(&self.rope, keep_going),
            Engine::Unavailable => {
                let _ = keep_going;
                true
            }
        }
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
            let event = EditEvent {
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
            };
            match &mut self.engine {
                #[cfg(feature = "tree-sitter")]
                Engine::TreeSitter(s) => s.apply_edit(&event),
                #[cfg(feature = "textmate")]
                Engine::TextMate(s) => s.apply_edit(&event),
                Engine::Unavailable => {
                    let _ = event;
                }
            }
        }
        Ok(())
    }

    /// The bracket pair at char `pos` (the bracket before the cursor or at it), as
    /// [open, close] char positions; empty when there is none or the engine has no tree.
    #[wasm_bindgen(js_name = matchingBracket)]
    pub fn matching_bracket(&self, pos: u32) -> Vec<u32> {
        #[cfg(feature = "tree-sitter")]
        if let Engine::TreeSitter(s) = &self.engine {
            let rope = &self.rope;
            let byte = rope.char_to_byte((pos as usize).min(rope.len_chars()));
            if let Some((a, b)) = s.matching_bracket(byte) {
                return vec![rope.byte_to_char(a) as u32, rope.byte_to_char(b) as u32];
            }
        }
        let _ = pos;
        Vec::new()
    }

    /// Regions worth folding from the syntax tree, as pairs of header line and last hidden
    /// line; empty when the engine has no tree.
    #[wasm_bindgen(js_name = foldingRanges)]
    pub fn folding_ranges(&self) -> Vec<u32> {
        #[cfg(feature = "tree-sitter")]
        if let Engine::TreeSitter(s) = &self.engine {
            return s.fold_ranges().into_iter().flat_map(|(a, b)| [a as u32, b as u32]).collect();
        }
        Vec::new()
    }

    /// True when this module answers bracket questions from a syntax tree.
    #[wasm_bindgen(js_name = hasTree)]
    pub fn has_tree(&self) -> bool {
        #[cfg(feature = "tree-sitter")]
        if let Engine::TreeSitter(_) = &self.engine {
            return true;
        }
        false
    }

    /// Highlights lines `first..end`, as quads: line, start column, end column (chars),
    /// style id. With `bracket_colors`, bracket tokens get depth colours on top.
    pub fn highlight(&self, first: u32, end: u32, bracket_colors: bool) -> Vec<u32> {
        // Unused in builds without tree-sitter, where the function returns early.
        let _ = bracket_colors;
        let lines = first as usize..end as usize;
        #[allow(unused_variables)]
        let highlights = match &self.engine {
            #[cfg(feature = "tree-sitter")]
            Engine::TreeSitter(s) => s.highlight(&self.rope, lines),
            #[cfg(feature = "textmate")]
            Engine::TextMate(s) => s.highlight(&self.rope, lines),
            Engine::Unavailable => {
                let _ = lines;
                return Vec::new();
            }
        };
        #[allow(unreachable_code)]
        let mut out = Vec::new();
        #[cfg(any(feature = "tree-sitter", feature = "textmate"))]
        for line in first..end {
            for span in highlights.line(line as usize) {
                out.extend([line, span.start as u32, span.end as u32, span.style]);
            }
        }
        #[cfg(feature = "tree-sitter")]
        if bracket_colors && let Engine::TreeSitter(s) = &self.engine {
            let rope = &self.rope;
            let lines = (first as usize).min(rope.len_lines())..(end as usize).min(rope.len_lines());
            let bytes = rope.line_to_byte(lines.start)..rope.line_to_byte(lines.end);
            for (byte, depth) in s.bracket_depths(bytes) {
                let c = rope.byte_to_char(byte);
                let line = rope.char_to_line(c);
                let col = c - rope.line_to_char(line);
                let style = rustaco_syntax::BRACKET_STYLE_BASE + depth as u32 % rustaco_syntax::BRACKET_COLORS;
                out.extend([line as u32, col as u32, col as u32 + 1, style]);
            }
        }
        out
    }

    /// Char ranges of the syntax nodes around `start..end`, innermost first, as pairs
    /// (start, end). Empty for engines without a tree (TextMate).
    #[wasm_bindgen(js_name = enclosingRanges)]
    pub fn enclosing_ranges(&self, start: u32, end: u32) -> Vec<u32> {
        #[allow(unused_mut)]
        let mut out = Vec::new();
        #[cfg(feature = "tree-sitter")]
        if let Engine::TreeSitter(s) = &self.engine {
            let rope = &self.rope;
            let len = rope.len_chars();
            let bytes = rope.char_to_byte((start as usize).min(len))..rope.char_to_byte((end as usize).min(len));
            for r in s.enclosing_ranges(bytes) {
                out.extend([rope.byte_to_char(r.start) as u32, rope.byte_to_char(r.end) as u32]);
            }
        }
        let _ = (start, end);
        out
    }

    /// 0 for code, 1 inside a string, 2 inside a comment, at char `pos`.
    #[wasm_bindgen(js_name = contextAt)]
    pub fn context_at(&self, pos: u32) -> u8 {
        let pos = (pos as usize).min(self.rope.len_chars());
        match &self.engine {
            #[cfg(feature = "tree-sitter")]
            Engine::TreeSitter(s) => s.context_at(self.rope.char_to_byte(pos)) as u8,
            #[cfg(feature = "textmate")]
            Engine::TextMate(s) => s.context_at(&self.rope, pos) as u8,
            Engine::Unavailable => {
                let _ = pos;
                0
            }
        }
    }

    /// The module's copy of the text; for tests that check it stays in step with the editor.
    pub fn text(&self) -> String {
        self.rope.to_string()
    }
}
