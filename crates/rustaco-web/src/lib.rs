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

use std::collections::HashMap;

use rustaco_core::{
    Cursors, Editor as Commands, EditorId, KeyPress, Keymap, Platform, Resolved, Rope, SearchQuery, Searcher,
    Selection, Selections, Stickiness, TextModel, default_keymap, jetbrains_keymap, next_match, word_at,
};
use rustaco_syntax::{Colors, Style, Theme, themes};
use rustaco_view::{Metrics, Rect, StyleSpan, Styler, View, indent_fold_ranges};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

mod lsp;
mod paint;
use paint::{Align, CanvasPainter, GlLayer, GlPainter, Painter};

/// Editor surface colours as CSS strings, from the current theme.
#[derive(Clone)]
struct Chrome {
    background: String,
    gutter: String,
    line_number: String,
    line_number_active: String,
    selection: String,
    current_line: String,
    caret: String,
    scrollbar: String,
    find_match: String,
    indent_guide: String,
    indent_guide_active: String,
    ruler: String,
    whitespace: String,
    bracket_match: String,
    word_highlight: String,
}

/// Decoration kind of find matches.
const FIND_MATCH: u32 = 1;

/// Decoration kind of the text an IME composition is building.
const COMPOSITION: u32 = 2;

/// Composition decorations belong to this owner plus the editor's id.
const COMPOSITION_OWNER: u32 = 1 << 19;

/// Decoration kinds from here on are types defined through [`define_decoration_type`].
const FIRST_DECORATION_TYPE: u32 = 100;

/// How a decoration type draws.
#[derive(Clone, Default)]
struct DecorationType {
    /// Fill behind the text (or the whole line).
    background: Option<String>,
    /// 0 none, 1 solid line, 2 wavy line (diagnostics), 3 dotted.
    underline: u8,
    underline_color: String,
    /// Covers whole lines instead of the range's chars.
    whole_line: bool,
    /// Bar in the gutter beside the line numbers.
    gutter: Option<String>,
    /// Mark in the overview ruler on the scrollbar.
    overview: Option<String>,
}

/// Defines how decorations of a new kind draw and returns the kind. Empty strings leave a
/// part out. `underline` is `"solid"`, `"wavy"`, `"dotted"` or empty. Types are shared by
/// every editor.
#[wasm_bindgen(js_name = defineDecorationType)]
pub fn define_decoration_type(
    background: &str,
    underline: &str,
    underline_color: &str,
    whole_line: bool,
    gutter: &str,
    overview: &str,
) -> u32 {
    let some = |s: &str| (!s.is_empty()).then(|| s.to_owned());
    let t = DecorationType {
        background: some(background),
        underline: match underline {
            "solid" => 1,
            "wavy" => 2,
            "dotted" => 3,
            _ => 0,
        },
        underline_color: underline_color.to_owned(),
        whole_line,
        gutter: some(gutter),
        overview: some(overview),
    };
    DECORATION_TYPES.with(|types| {
        let mut types = types.borrow_mut();
        types.push(t);
        FIRST_DECORATION_TYPE + types.len() as u32 - 1
    })
}

fn css(rgba: u32) -> String {
    format!("#{rgba:08x}")
}

impl Chrome {
    fn from(c: &Colors) -> Self {
        Chrome {
            background: css(c.background),
            gutter: css(c.gutter),
            line_number: css(c.line_number),
            line_number_active: css(c.line_number_active),
            selection: css(c.selection),
            current_line: css(c.current_line),
            caret: css(c.caret),
            scrollbar: css(c.scrollbar),
            find_match: css(c.find_match),
            indent_guide: css(c.indent_guide),
            indent_guide_active: css(c.indent_guide_active),
            ruler: css(c.ruler),
            whitespace: css(c.whitespace),
            bracket_match: css(c.bracket_match),
            word_highlight: css(c.word_highlight),
        }
    }
}

#[wasm_bindgen(inline_js = "
export function draw_blocks(ctx, ops, colors) {
  const c = colors.split('\\n');
  let color = -1;
  for (let i = 0; i < ops.length; i += 5) {
    if (ops[i + 4] !== color) { color = ops[i + 4]; ctx.fillStyle = c[color]; }
    ctx.fillRect(ops[i], ops[i + 1], ops[i + 2], ops[i + 3]);
  }
}
export function draw_runs(ctx, text, ops, colors, fonts) {
  const c = colors.split('\\n'), f = fonts.split('\\n');
  let font = -1, color = -1;
  for (let i = 0; i < ops.length; i += 7) {
    if (ops[i + 5] !== font) { font = ops[i + 5]; ctx.font = f[font]; }
    if (ops[i + 4] !== color) { color = ops[i + 4]; ctx.fillStyle = c[color]; }
    const run = text.substring(ops[i + 2], ops[i + 3]);
    ctx.fillText(run, ops[i], ops[i + 1]);
    if (ops[i + 6] > 0) {
      ctx.fillRect(ops[i], ops[i + 1] + ops[i + 6], ctx.measureText(run).width, 1);
    }
  }
}")]
extern "C" {
    /// Fills rectangles: `ops` holds five numbers per rectangle, x, y, width, height and a
    /// colour index into the newline-separated `colors`.
    fn draw_blocks(ctx: &CanvasRenderingContext2d, ops: &[f64], colors: &str);

    /// Draws text runs. `ops` holds seven numbers per run: x, y, start and end of the run in
    /// `text` (UTF-16 units), colour index and font index into the newline-separated lists,
    /// and the underline offset below the text's middle (0 for no underline).
    fn draw_runs(ctx: &CanvasRenderingContext2d, text: &str, ops: &[f64], colors: &str, fonts: &str);
}

thread_local! {
    static NEXT_EDITOR_ID: Cell<u32> = const { Cell::new(1) };
    /// Per style id: CSS colour, font prefix and underline, from the current theme. Shared
    /// by all editors, like the surface colours.
    static STYLES: RefCell<Vec<CssStyle>> = RefCell::new(css_styles(&themes::dark()));
    static CHROME: RefCell<Chrome> = RefCell::new(Chrome::from(&Colors::DARK));
    static DECORATION_TYPES: RefCell<Vec<DecorationType>> = const { RefCell::new(Vec::new()) };
    static WIDGET_COLORS: RefCell<Colors> = const { RefCell::new(Colors::DARK) };
}

#[derive(Clone)]
struct CssStyle {
    color: String,
    /// `"italic bold "` or a part of it, prepended to the font size and family.
    font_prefix: &'static str,
    underline: bool,
}

fn css_styles(theme: &Theme) -> Vec<CssStyle> {
    let brackets = theme.colors.brackets.map(|c| Style { foreground: Some(c), ..Style::default() });
    theme
        .style_table()
        .iter()
        // Bracket depth colours follow the highlight names (style ids from BRACKET_STYLE_BASE).
        .chain(brackets.iter())
        .map(|s: &Style| CssStyle {
            color: css(s.foreground.unwrap_or(theme.colors.foreground)),
            font_prefix: match (s.italic.unwrap_or(false), s.bold.unwrap_or(false)) {
                (true, true) => "italic bold ",
                (true, false) => "italic ",
                (false, true) => "bold ",
                (false, false) => "",
            },
            underline: s.underline.unwrap_or(false),
        })
        .collect()
}

fn apply_theme(theme: &Theme) {
    STYLES.with(|s| *s.borrow_mut() = css_styles(theme));
    CHROME.with(|c| *c.borrow_mut() = Chrome::from(&theme.colors));
    WIDGET_COLORS.with(|c| *c.borrow_mut() = theme.colors);
}

/// Colours for DOM widgets drawn over the editor (the find widget, a host's hover boxes):
/// background, foreground, border, input background and the find match colour, as CSS.
#[wasm_bindgen(js_name = widgetColors)]
pub fn widget_colors() -> Vec<String> {
    let colors = WIDGET_COLORS.with(|c| *c.borrow());
    [colors.current_line, colors.foreground, colors.indent_guide_active, colors.background, colors.find_match]
        .map(|c| format!("#{:06x}", c >> 8))
        .to_vec()
}

/// The style id of a highlight name (`"function"`, `"variable.parameter"`, ...), for
/// semantic tokens; names without a style get the closest broader one, or 0.
#[wasm_bindgen(js_name = styleId)]
pub fn style_id(name: &str) -> u32 {
    rustaco_syntax::style_for_capture(name)
}

/// Hover and documentation markdown as blocks (see `rustaco_view::markdown`), as JSON:
/// `[["pre", text] | ["p", [[kind, text], ...]]]` with kind `text`, `code`, `b` or `i`.
#[wasm_bindgen(js_name = parseMarkdown)]
pub fn parse_markdown(text: &str) -> String {
    use rustaco_view::markdown::{Block, Inline};
    let blocks: Vec<serde_json::Value> = rustaco_view::markdown::parse(text)
        .into_iter()
        .map(|b| match b {
            Block::Code(code) => serde_json::json!(["pre", code]),
            Block::Paragraph(pieces) => {
                let pieces: Vec<serde_json::Value> = pieces
                    .into_iter()
                    .map(|p| match p {
                        Inline::Text(t) => serde_json::json!(["text", t]),
                        Inline::Code(t) => serde_json::json!(["code", t]),
                        Inline::Bold(t) => serde_json::json!(["b", t]),
                        Inline::Italic(t) => serde_json::json!(["i", t]),
                    })
                    .collect();
                serde_json::json!(["p", pieces])
            }
        })
        .collect();
    serde_json::Value::Array(blocks).to_string()
}

/// Selects a bundled theme: `"dark"`, `"light"` or `"high-contrast"`.
#[wasm_bindgen(js_name = setBuiltinTheme)]
pub fn set_builtin_theme(name: &str) -> bool {
    let theme = match name {
        "dark" => themes::dark(),
        "light" => themes::light(),
        "high-contrast" => themes::high_contrast(),
        _ => return false,
    };
    apply_theme(&theme);
    true
}

/// Sets a VS Code colour theme from its JSON text (see `rustaco_syntax::Theme::from_vscode`).
#[wasm_bindgen(js_name = setVsCodeTheme)]
pub fn set_vscode_theme(json: &str) -> Result<(), JsValue> {
    let theme = Theme::from_vscode(json).map_err(|e| JsValue::from_str(&e))?;
    apply_theme(&theme);
    Ok(())
}

/// A text model that one or more editors can show.
/// Diffs two texts and flattens the result for JS: a completeness flag (1 or 0), the change
/// count, then per change the old line range, the new line range, the moved partner plus
/// one (0 for none), the number of char changes, and four offsets per char change (old start,
/// old end, new start, new end).
fn encode_diff(old: &Rope, new: &Rope, ignore_whitespace: bool, budget_ms: f64) -> Vec<u32> {
    let deadline = js_sys::Date::now() + budget_ms;
    let mut keep_going = || js_sys::Date::now() < deadline;
    let d = rustaco_core::diff(old, new, rustaco_core::DiffOptions { ignore_whitespace }, &mut keep_going);
    let mut out = vec![u32::from(d.complete), d.changes.len() as u32];
    for c in &d.changes {
        out.extend([c.old.start, c.old.end, c.new.start, c.new.end].map(|n| n as u32));
        out.push(c.moved.map_or(0, |m| m as u32 + 1));
        out.push(c.inner.len() as u32);
        for i in &c.inner {
            out.extend([i.old.start, i.old.end, i.new.start, i.new.end].map(|n| n as u32));
        }
    }
    out
}

/// The changes of a flat diff from [`encode_diff`].
fn decode_diff(flat: &[u32]) -> Vec<rustaco_core::LineChange> {
    let n = |i: usize| flat.get(i).copied().unwrap_or(0) as usize;
    let mut out = Vec::new();
    let mut i = 2;
    for _ in 0..n(1) {
        let mut c = rustaco_core::LineChange {
            old: n(i)..n(i + 1),
            new: n(i + 2)..n(i + 3),
            inner: Vec::new(),
            moved: n(i + 4).checked_sub(1),
        };
        let count = n(i + 5);
        i += 6;
        for _ in 0..count {
            c.inner.push(rustaco_core::CharChange { old: n(i)..n(i + 1), new: n(i + 2)..n(i + 3) });
            i += 4;
        }
        out.push(c);
    }
    out
}

/// Filler blocks for a side-by-side diff (see `rustaco_core::diff_view::fillers`), three
/// numbers each: 1 for the modified pane or 0 for the original, the line the block follows
/// (-1: before the first) and its height in lines.
#[wasm_bindgen(js_name = diffFillers)]
pub fn diff_fillers(flat: &[u32]) -> Vec<i32> {
    rustaco_core::diff_view::fillers(&decode_diff(flat))
        .into_iter()
        .flat_map(|f| [i32::from(f.in_modified), f.after_line as i32, f.lines as i32])
        .collect()
}

/// The first line of the next (or previous) change around `line` of the modified text, or
/// -1 without changes.
#[wasm_bindgen(js_name = diffNextChange)]
pub fn diff_next_change(flat: &[u32], line: u32, forward: bool) -> i32 {
    rustaco_core::diff_view::next_change(&decode_diff(flat), line as usize, forward).map_or(-1, |l| l as i32)
}

/// Unchanged stretches to fold on one side, as pairs of first and last line.
#[wasm_bindgen(js_name = diffUnchangedRegions)]
pub fn diff_unchanged_regions(flat: &[u32], in_modified: bool, line_count: u32, context: u32) -> Vec<u32> {
    rustaco_core::diff_view::unchanged_regions(&decode_diff(flat), in_modified, line_count as usize, context as usize)
        .into_iter()
        .flat_map(|(a, b)| [a as u32, b as u32])
        .collect()
}

#[wasm_bindgen]
impl Model {
    /// The ranges a diff view marks, this model being the original: triples of kind (0
    /// removed line, 1 inserted line, 2 removed chars, 3 inserted chars), start and end. The
    /// removed kinds are offsets into this model, the inserted ones into `modified`.
    #[wasm_bindgen(js_name = diffMarks)]
    pub fn diff_marks(&self, flat: &[u32], modified: &Model, inline: bool) -> Vec<u32> {
        use rustaco_core::diff_view::{DiffMark, marks};
        let (a, b) = (self.inner.borrow(), modified.inner.borrow());
        marks(a.rope(), b.rope(), &decode_diff(flat), inline)
            .into_iter()
            .flat_map(|(kind, r)| {
                let k = match kind {
                    DiffMark::RemovedLine => 0,
                    DiffMark::InsertedLine => 1,
                    DiffMark::RemovedText => 2,
                    DiffMark::InsertedText => 3,
                };
                [k, r.start as u32, r.end as u32]
            })
            .collect()
    }

    /// The edit to `modified` that reverts change `index`, this model being the original:
    /// JSON `{ start, end, text }`, or `null` for a missing change.
    #[wasm_bindgen(js_name = diffRevertEdit)]
    pub fn diff_revert_edit(&self, flat: &[u32], index: u32, modified: &Model) -> String {
        let changes = decode_diff(flat);
        let Some(c) = changes.get(index as usize) else { return "null".into() };
        let (a, b) = (self.inner.borrow(), modified.inner.borrow());
        let (range, text) = rustaco_core::diff_view::revert_edit(a.rope(), b.rope(), c);
        serde_json::json!({ "start": range.start, "end": range.end, "text": text }).to_string()
    }
}

/// CSS colours of every style id under the current theme, for DOM that shows highlighted
/// text (the deleted lines of an inline diff).
#[wasm_bindgen(js_name = styleColors)]
pub fn style_colors() -> Vec<String> {
    STYLES.with(|s| s.borrow().iter().map(|c| c.color.clone()).collect())
}

/// Most matches a search in the worker returns.
const MATCHES_LIMIT: usize = 19_999;

/// A copy of a model's text that follows its edits, for work in a worker (search).
#[wasm_bindgen]
pub struct TextReplica {
    rope: Rope,
}

#[wasm_bindgen]
impl TextReplica {
    #[wasm_bindgen(constructor)]
    pub fn new(text: &str) -> TextReplica {
        TextReplica { rope: Rope::from_str(text) }
    }

    pub fn reset(&mut self, text: &str) {
        self.rope = Rope::from_str(text);
    }

    /// Applies an edit batch as [`EditBatch`] numbers it: nine numbers per edit (start, old
    /// end and new end in bytes, then points), the inserted texts concatenated.
    pub fn edit(&mut self, numbers: &[u32], text: &str) -> Result<(), JsValue> {
        let mut offset = 0;
        for e in numbers.chunks_exact(9) {
            let (start, old_end, new_end) = (e[0] as usize, e[1] as usize, e[2] as usize);
            if start > old_end || old_end > self.rope.len_bytes() || new_end < start {
                return Err(JsValue::from_str("edit outside the text"));
            }
            let inserted = text
                .get(offset..offset + (new_end - start))
                .ok_or_else(|| JsValue::from_str("edit text out of step"))?;
            offset += inserted.len();
            let (a, b) = (self.rope.byte_to_char(start), self.rope.byte_to_char(old_end));
            self.rope.remove(a..b);
            self.rope.insert(a, inserted);
        }
        Ok(())
    }

    /// Diffs this text (the original) against `modified`. The result is flat: a completeness
    /// flag, the change count, then per change the old and new line ranges, the moved partner
    /// plus one, the char change count and four offsets per char change.
    /// After `budget_ms` the rest is diffed coarsely.
    pub fn diff(&self, modified: &TextReplica, ignore_whitespace: bool, budget_ms: f64) -> Vec<u32> {
        encode_diff(&self.rope, &modified.rope, ignore_whitespace, budget_ms)
    }

    /// Every match of the query as pairs of start and end char offsets, or undefined when the
    /// regular expression does not compile.
    #[wasm_bindgen(js_name = findAll)]
    pub fn find_all(&self, pattern: &str, regex: bool, case_sensitive: bool, whole_word: bool) -> Option<Vec<u32>> {
        let query = SearchQuery { pattern: pattern.to_owned(), regex, case_sensitive, whole_word };
        let searcher = Searcher::new(&query).ok()?;
        if pattern.is_empty() {
            return Some(Vec::new());
        }
        // Like Monaco, at most 19999 matches: past that the highlighting would cost more than
        // it tells.
        let matches = searcher.find_all(&self.rope, None);
        Some(matches.into_iter().take(MATCHES_LIMIT).flat_map(|r| [r.start as u32, r.end as u32]).collect())
    }
}

#[wasm_bindgen]
pub struct Model {
    inner: Rc<RefCell<TextModel>>,
    /// Words collected for word completion: caret line, time collected (ms) and the words.
    words: RefCell<Option<(usize, f64, Vec<String>)>>,
}

impl Model {
    /// Distinct words from `radius` lines on each side of `line`, at most `max`, without
    /// `exclude` (the word being typed). They are kept for five seconds while the line stays
    /// within a quarter of the radius: collecting reads thousands of lines and the words
    /// change little between keystrokes.
    fn words_near(&self, line: usize, radius: usize, max: usize, exclude: &str) -> Vec<String> {
        let now = js_sys::Date::now();
        let mut cache = self.words.borrow_mut();
        let fresh = cache.as_ref().is_some_and(|(at, t, _)| at.abs_diff(line) < radius / 4 && now - t < 5000.0);
        if !fresh {
            let model = self.inner.borrow();
            let words = rustaco_core::collect_words(model.rope(), line.saturating_sub(radius)..line + radius + 1, max);
            *cache = Some((line, now, words));
        }
        cache.as_ref().expect("filled above").2.iter().filter(|w| *w != exclude).cloned().collect()
    }
}

#[wasm_bindgen]
impl Model {
    #[wasm_bindgen(constructor)]
    pub fn new(text: &str) -> Model {
        Model { inner: Rc::new(RefCell::new(TextModel::new(text))), words: RefCell::new(None) }
    }

    /// Diffs this model (the original) against `modified` on the calling thread; the result
    /// is laid out as `TextReplica.diff` returns it.
    #[wasm_bindgen(js_name = diffWith)]
    pub fn diff_with(&self, modified: &Model, ignore_whitespace: bool, budget_ms: f64) -> Vec<u32> {
        let (a, b) = (self.inner.borrow(), modified.inner.borrow());
        encode_diff(a.rope(), b.rope(), ignore_whitespace, budget_ms)
    }

    /// The text between two char offsets.
    #[wasm_bindgen(js_name = getTextRange)]
    pub fn get_text_range(&self, start: u32, end: u32) -> String {
        let model = self.inner.borrow();
        let len = model.len_chars();
        let end = (end as usize).min(len);
        model.rope().slice((start as usize).min(end)..end).to_string()
    }

    /// Line and column (0-based, in chars) of a char offset.
    #[wasm_bindgen(js_name = positionAt)]
    pub fn position_at(&self, offset: u32) -> Vec<u32> {
        let model = self.inner.borrow();
        let p = rustaco_core::char_to_position(model.rope(), offset as usize);
        vec![p.line as u32, p.column as u32]
    }

    /// The char offset of a line and column (0-based), clamped into the text.
    #[wasm_bindgen(js_name = offsetAt)]
    pub fn offset_at(&self, line: u32, column: u32) -> u32 {
        let model = self.inner.borrow();
        let rope = model.rope();
        let line = (line as usize).min(rope.len_lines() - 1);
        (rope.line_to_char(line) + (column as usize).min(rustaco_core::line_len(rope, line))) as u32
    }

    #[wasm_bindgen(js_name = lineCount)]
    pub fn line_count(&self) -> u32 {
        self.inner.borrow().rope().len_lines() as u32
    }

    /// Length of the text in chars (JS's `length` counts UTF-16 units).
    #[wasm_bindgen(js_name = charCount)]
    pub fn char_count(&self) -> u32 {
        self.inner.borrow().len_chars() as u32
    }

    /// Rows for a list of references: `ranges` holds start and end char offsets per range.
    /// Answers JSON, sorted by start: `[{ start, end, line, pre, match, post }]`, where `pre`
    /// is the line before the match without its indentation and `post` the rest of the line.
    #[wasm_bindgen(js_name = referenceRows)]
    pub fn reference_rows(&self, ranges: &[u32]) -> String {
        let model = self.inner.borrow();
        let rope = model.rope();
        let len = rope.len_chars();
        let mut pairs: Vec<(usize, usize)> =
            ranges.chunks_exact(2).map(|r| ((r[0] as usize).min(len), (r[1] as usize).min(len))).collect();
        pairs.sort_unstable();
        let rows: Vec<serde_json::Value> = pairs
            .into_iter()
            .map(|(start, end)| {
                let line = rope.char_to_line(start);
                let line_start = rope.line_to_char(line);
                let line_end = line_start + rustaco_core::line_len(rope, line);
                let end_on_line = end.clamp(start, line_end);
                let text = |a: usize, b: usize| rope.slice(a..b).to_string();
                serde_json::json!({
                    "start": start,
                    "end": end,
                    "line": line,
                    "pre": text(line_start, start).trim_start(),
                    "match": text(start, end_on_line),
                    "post": text(end_on_line, line_end),
                })
            })
            .collect();
        serde_json::Value::Array(rows).to_string()
    }

    /// An outline tree from a language module's flat symbols (six numbers each: start, end,
    /// name start, name end, kind, depth), as JSON:
    /// `[{ name, kind, range: { start, end }, selectionRange, children }]`.
    #[wasm_bindgen(js_name = symbolTree)]
    pub fn symbol_tree(&self, flat: &[u32]) -> String {
        use serde_json::{Value, json};
        let model = self.inner.borrow();
        let rope = model.rope();
        let len = rope.len_chars();
        fn take(rope: &rustaco_core::Rope, len: usize, flat: &[&[u32]], i: &mut usize, depth: u32) -> Vec<Value> {
            let mut out = Vec::new();
            while *i < flat.len() && flat[*i][5] >= depth {
                let s = flat[*i];
                *i += 1;
                let children = take(rope, len, flat, i, s[5] + 1);
                let (ns, ne) = ((s[2] as usize).min(len), (s[3] as usize).min(len));
                out.push(json!({
                    "name": rope.slice(ns.min(ne)..ne).to_string(),
                    "kind": s[4],
                    "range": { "start": s[0], "end": s[1] },
                    "selectionRange": { "start": s[2], "end": s[3] },
                    "children": children,
                }));
            }
            out
        }
        let rows: Vec<&[u32]> = flat.chunks_exact(6).collect();
        Value::Array(take(rope, len, &rows, &mut 0, 0)).to_string()
    }

    /// Replaces the decorations of `owner` (an id the caller picks, at or above 2^20 so it
    /// stays clear of editors' own) with triples of start, end (char offsets) and kind from
    /// [`define_decoration_type`]. Every editor showing the model draws them; they move with
    /// edits.
    #[wasm_bindgen(js_name = setDecorations)]
    pub fn set_decorations(&self, owner: u32, triples: &[u32]) {
        let mut model = self.inner.borrow_mut();
        let len = model.len_chars();
        let items = triples.chunks_exact(3).map(|t| {
            let end = (t[1] as usize).min(len);
            ((t[0] as usize).min(end)..end, t[2])
        });
        model.decorations.set_each(owner, Stickiness::NeverGrows, items);
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

    /// Sets comment tokens and auto-closing pairs from the language catalog; `""` clears them.
    #[wasm_bindgen(js_name = setLanguageConfig)]
    pub fn set_language_config(&self, language: &str) {
        let editing = rustaco_syntax::language_editing(language);
        self.inner.borrow_mut().editing = editing;
    }

    /// Edits made after `version`, encoded for a language module (see [`EditBatch`]).
    #[wasm_bindgen(js_name = editsSince)]
    pub fn edits_since(&self, version: f64) -> EditBatch {
        let model = self.inner.borrow();
        let mut batch =
            EditBatch { numbers: Vec::new(), text: String::new(), stale: false, utf16: Vec::new(), texts: Vec::new() };
        match model.events_since(version as u64) {
            Ok(events) => {
                for ev in events {
                    batch.numbers.extend(
                        [
                            ev.start_byte,
                            ev.old_end_byte,
                            ev.new_end_byte,
                            ev.start_point.row,
                            ev.start_point.column,
                            ev.old_end_point.row,
                            ev.old_end_point.column,
                            ev.new_end_point.row,
                            ev.new_end_point.column,
                        ]
                        .map(|n| n as u32),
                    );
                    batch.text.push_str(&ev.text);
                    batch.utf16.extend(
                        [
                            ev.start_utf16.line,
                            ev.start_utf16.character,
                            ev.old_end_utf16.line,
                            ev.old_end_utf16.character,
                        ]
                        .map(|n| n as u32),
                    );
                    batch.texts.push(ev.text.clone());
                }
            }
            Err(_) => batch.stale = true,
        }
        batch
    }

    /// Language id for a file path and first line, or `""` when none matches.
    #[wasm_bindgen(js_name = detectLanguage)]
    pub fn detect_language(path: &str, first_line: &str) -> String {
        rustaco_syntax::detect(path, first_line).map_or_else(String::new, |l| l.id.to_owned())
    }

    /// The language module (pack) that parses `language`, or `""` for an unknown id.
    #[wasm_bindgen(js_name = languagePack)]
    pub fn language_pack(language: &str) -> String {
        rustaco_syntax::info(language).map_or_else(String::new, |l| l.pack.to_owned())
    }

    /// Ids of every language the editor knows.
    #[wasm_bindgen(js_name = knownLanguages)]
    pub fn known_languages() -> Vec<String> {
        rustaco_syntax::catalog().iter().map(|l| l.id.to_owned()).collect()
    }
}

/// Edits for a language module: `numbers` holds 9 values per edit (start, old end, new end
/// in bytes; rows and byte columns of the same three points), `text` the inserted texts
/// concatenated. `stale` means the model's log no longer reaches back to the requested
/// version, and the module must be reset with the full text.
#[wasm_bindgen]
pub struct EditBatch {
    numbers: Vec<u32>,
    text: String,
    stale: bool,
    /// Per edit: start line and UTF-16 character, old end line and UTF-16 character, as
    /// LSP's incremental `didChange` takes them.
    utf16: Vec<u32>,
    /// The inserted text of each edit.
    texts: Vec<String>,
}

#[wasm_bindgen]
impl EditBatch {
    pub fn numbers(&self) -> Vec<u32> {
        self.numbers.clone()
    }

    pub fn text(&self) -> String {
        self.text.clone()
    }

    pub fn stale(&self) -> bool {
        self.stale
    }

    /// Four numbers per edit: start line, start UTF-16 character, old end line, old end
    /// UTF-16 character; each relative to the text after the edits before it.
    pub fn utf16(&self) -> Vec<u32> {
        self.utf16.clone()
    }

    /// The inserted text of each edit.
    pub fn texts(&self) -> Vec<String> {
        self.texts.clone()
    }
}

/// Style spans for the lines last highlighted by a language module. They move with edits
/// until fresh ones arrive, so a line inserted above does not shift the colours of every
/// line below for a frame (highlights from a worker come a frame or two late).
#[derive(Default)]
struct ExternalHighlights {
    lines: std::collections::BTreeMap<usize, Vec<StyleSpan>>,
    /// Semantic tokens from a provider, drawn over the syntax highlights.
    semantic: std::collections::BTreeMap<usize, Vec<StyleSpan>>,
    /// Model version the line numbers refer to.
    version: u64,
    /// Set when new highlights or a new bracket pair differ from what is drawn; the JS side
    /// skips the frame otherwise.
    changed: bool,
}

impl ExternalHighlights {
    /// Moves the lines through the edits made since they were set.
    fn sync(&mut self, model: &TextModel) {
        if self.version == model.version() {
            return;
        }
        match model.events_since(self.version) {
            Ok(events) => {
                for e in events {
                    let a = e.start_point.row;
                    let old = e.old_end_point.row - a;
                    let new = e.new_end_point.row - a;
                    if old == 0 && new == 0 {
                        // An edit inside one line: move its spans by the chars added or
                        // removed, so they match what the language module will send for
                        // the new text (and the frame for that reply can be skipped). The
                        // UTF-16 column stands in for the char column; they differ only
                        // after chars outside the BMP, and the reply corrects that.
                        let col = e.start_utf16.character;
                        let removed = e.old_end - e.start;
                        let added = e.new_end - e.start;
                        for map in [&mut self.lines, &mut self.semantic] {
                            if let Some(spans) = map.get_mut(&a) {
                                shift_spans(spans, col, removed, added);
                            }
                        }
                        continue;
                    }
                    for map in [&mut self.lines, &mut self.semantic] {
                        let moved = map.split_off(&(a + 1));
                        for (line, spans) in moved {
                            if line > a + old {
                                map.insert(line + new - old, spans);
                            }
                        }
                    }
                }
            }
            Err(_) => {
                self.lines.clear();
                self.semantic.clear();
            }
        }
        self.version = model.version();
    }
}

/// Moves `spans` (sorted char columns of one line) through an edit at `col` that replaced
/// `removed` chars with `added`: spans after it move, a span around it grows or shrinks,
/// spans inside the removed text disappear.
fn shift_spans(spans: &mut Vec<StyleSpan>, col: usize, removed: usize, added: usize) {
    let end = col + removed;
    let map = |x: usize| {
        if x <= col {
            x
        } else if x >= end {
            x - removed + added
        } else {
            col + added.min(x - col)
        }
    };
    for s in spans.iter_mut() {
        // An insertion at a span's end extends the span (typing at the end of a word).
        let grow_end = removed == 0 && s.end == col && s.start < col;
        s.start = if s.start < col {
            s.start
        } else if s.start >= end {
            s.start - removed + added
        } else {
            col + added
        };
        s.end = if grow_end { s.end + added } else { map(s.end) };
    }
    spans.retain(|s| s.start < s.end);
}

impl Styler for ExternalHighlights {
    fn spans(&self, line: usize) -> Vec<StyleSpan> {
        let base = self.lines.get(&line).cloned().unwrap_or_default();
        match self.semantic.get(&line) {
            Some(over) => overlay_spans(&base, over),
            None => base,
        }
    }
}

/// `base` spans with `over` drawn on top: where they overlap, `over` wins. Both sorted and
/// disjoint; so is the result.
fn overlay_spans(base: &[StyleSpan], over: &[StyleSpan]) -> Vec<StyleSpan> {
    let mut out: Vec<StyleSpan> = Vec::with_capacity(base.len() + over.len());
    for b in base {
        let mut start = b.start;
        for o in over.iter().filter(|o| o.start < b.end && o.end > b.start) {
            if o.start > start {
                out.push(StyleSpan { start, end: o.start, style: b.style });
            }
            start = start.max(o.end);
        }
        if start < b.end {
            out.push(StyleSpan { start, end: b.end, style: b.style });
        }
    }
    out.extend_from_slice(over);
    out.sort_by_key(|s| s.start);
    out
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
    /// Shift+Alt-drag selects a rectangle: the anchor's line and visual column.
    column: Option<(usize, f32)>,
}

#[wasm_bindgen]
pub struct Editor {
    model: Rc<RefCell<TextModel>>,
    highlights: ExternalHighlights,
    /// What the last [`Editor::highlight_request`] asked about: version, runs, caret, colours.
    highlight_key: Option<(u64, Vec<u32>, u32, bool)>,
    cursors: Cursors,
    view: View,
    canvas: HtmlCanvasElement,
    ctx: CanvasRenderingContext2d,
    font: String,
    dpr: f64,
    drag: Option<Drag>,
    caret: Rect,
    keymap: Keymap,
    platform: Platform,
    /// The current find query, kept for F3 navigation and for refreshing matches.
    find: Option<(SearchQuery, Searcher)>,
    /// A command the keymap resolved that only the JS side can run (it needs the language
    /// module); [`Editor::key_down`] returns 2 and the JS side takes it from here.
    frontend_command: Option<String>,
    options: ViewOptions,
    /// The bracket pair at the cursor from the language module's tree, set each frame.
    tree_brackets: Option<Option<(usize, usize)>>,
    /// Minimap slider drag in progress: pointer y and scroll top when it started.
    minimap_drag: Option<(f64, f64)>,
    /// Sticky scroll lines of the last frame: y, height and line.
    sticky: Vec<(f32, f32, usize)>,
    /// Overview ruler marks, kept until [`Editor::refresh_overview`].
    overview: Option<Vec<(f32, f32, u32)>>,
    /// Scrollbar thumb drag in progress: vertical or not, pointer position and scroll
    /// position when it started, scroll distance per pixel.
    scrollbar_drag: Option<(bool, f64, f64, f64)>,
    /// Dragging selected text: the selected range and, once the pointer has moved, where
    /// it would drop.
    text_drag: Option<(std::ops::Range<usize>, Option<usize>, f64, f64)>,
    /// View zones of the last frame: id and rectangle.
    zone_rects: Vec<(u32, Rect)>,
    /// The open completion list: the text each item is filtered by and its sort text.
    completion_items: (Vec<String>, Vec<String>),
    /// The items' labels, when they differ from the filter texts.
    completion_labels: Vec<String>,
    /// Commands the embedder added (`editor.addCommand`): keybindings resolving to them go
    /// to the JS side, like the frontend commands.
    host_commands: std::collections::HashSet<String>,
    /// The WebGL layer, when JS set one up: frames are then painted as quads for WebGL
    /// instead of on the canvas.
    gl: Option<GlLayer>,
}

/// Display options (Monaco's names in the JS API).
#[derive(Clone, Default)]
struct ViewOptions {
    rulers: Vec<usize>,
    render_whitespace: bool,
    guides: bool,
    occurrences: bool,
}

/// Commands the JS side runs because they need the language module.
const FRONTEND_COMMANDS: &[&str] = &[
    "editor.action.smartSelect.expand",
    "editor.action.jumpToBracket",
    "actions.find",
    "editor.action.startFindReplaceAction",
    "editor.action.triggerSuggest",
    "editor.action.triggerParameterHints",
    "editor.action.revealDefinition",
    "editor.action.referenceSearch.trigger",
    "editor.action.rename",
    "editor.action.formatDocument",
    "editor.action.showHover",
    "editor.action.quickFix",
    "editor.action.quickOutline",
];

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
        let metrics = Metrics::new(char_width, (font_size * 1.5).round() as f32);
        let id = EditorId(NEXT_EDITOR_ID.with(|n| n.replace(n.get() + 1)));
        let cursors = Cursors::new(id, &model.inner.borrow());
        Ok(Editor {
            model: model.inner.clone(),
            highlights: ExternalHighlights::default(),
            highlight_key: None,
            cursors,
            view: View::new(metrics, canvas.client_width() as f32, canvas.client_height() as f32),
            canvas,
            ctx,
            font,
            dpr: 1.0,
            drag: None,
            caret: Rect::default(),
            keymap: default_keymap(Platform::Windows),
            platform: Platform::Windows,
            find: None,
            frontend_command: None,
            options: ViewOptions { guides: true, occurrences: true, ..ViewOptions::default() },
            tree_brackets: None,
            minimap_drag: None,
            sticky: Vec::new(),
            overview: None,
            scrollbar_drag: None,
            text_drag: None,
            zone_rects: Vec::new(),
            completion_items: (Vec::new(), Vec::new()),
            completion_labels: Vec::new(),
            host_commands: std::collections::HashSet::new(),
            gl: None,
        })
    }

    /// Paints frames with WebGL (`"webgl"`) or on the canvas (`"canvas"`, the default). With
    /// WebGL, JS draws each frame from [`Editor::gl_instances`] after [`Editor::render`].
    #[wasm_bindgen(js_name = setTextLayer)]
    pub fn set_text_layer(&mut self, kind: &str) {
        self.gl = (kind == "webgl").then(GlLayer::default);
    }

    /// Quads of the last frame: pointer into WASM memory and count (eight 32-bit words each:
    /// x, y, width, height, slot origin x and y as f32 device pixels, atlas slot, RGBA).
    #[wasm_bindgen(js_name = glInstances)]
    pub fn gl_instances(&self) -> Vec<u32> {
        match &self.gl {
            Some(g) => vec![g.instances.as_ptr() as u32, (g.instances.len() / paint::INSTANCE_WORDS) as u32],
            None => vec![0, 0],
        }
    }

    /// Slot width, height, left padding and slots per row of the atlas, the atlas side, and 1
    /// when the atlas was cleared this frame.
    #[wasm_bindgen(js_name = glGeometry)]
    pub fn gl_geometry(&self) -> Vec<u32> {
        match &self.gl {
            Some(g) => {
                let (w, h, pad, cols) = g.geometry;
                vec![w, h, pad, cols, paint::ATLAS, g.reset as u32]
            }
            None => Vec::new(),
        }
    }

    /// Glyphs to draw into the atlas (slot, font variant and quarter-pixel offset per glyph),
    /// taken: the next call returns only glyphs laid out since. Read [`Editor::gl_geometry`]
    /// and [`Editor::gl_pending_chars`] first, as this clears the reset flag and the chars.
    #[wasm_bindgen(js_name = glTakePending)]
    pub fn gl_take_pending(&mut self) -> Vec<u32> {
        self.gl.as_mut().map(GlLayer::take_pending).unwrap_or_default()
    }

    /// The chars of the pending glyphs, one per glyph.
    #[wasm_bindgen(js_name = glPendingChars)]
    pub fn gl_pending_chars(&self) -> String {
        self.gl.as_ref().map(|g| g.pending_chars.clone()).unwrap_or_default()
    }

    /// Makes the next frame clear the atlas and draw its glyphs again (after the WebGL
    /// context was restored).
    #[wasm_bindgen(js_name = glForgetAtlas)]
    pub fn gl_forget_atlas(&mut self) {
        if let Some(g) = &mut self.gl {
            g.forget_atlas();
        }
    }

    /// Changes the font. The cell width is measured again, the line height follows the size,
    /// wrapping and line heights are recomputed at the next frame, the glyph atlas starts
    /// over, and the line at the top of the viewport stays there. Text sits on a grid of equal
    /// cells, so only monospace fonts line up.
    #[wasm_bindgen(js_name = setFont)]
    pub fn set_font(&mut self, font_family: &str, font_size: f64) -> Result<(), JsValue> {
        let font = format!("{font_size}px {font_family}");
        self.ctx.set_font(&font);
        let sample = "M".repeat(64);
        let char_width = (self.ctx.measure_text(&sample)?.width() / 64.0) as f32;
        let model = self.model.clone();
        let model = model.borrow();
        let rope = model.rope();
        let top_line = self.view.line_at_y(rope, self.view.scroll_top());
        let left_cells = self.view.scroll_left() / self.view.metrics.char_width;
        self.view.metrics = Metrics::new(char_width, (font_size * 1.5).round() as f32);
        self.font = font;
        self.view.sync(&model);
        let top = self.view.line_top(top_line);
        self.view.scroll_to(rope, top, left_cells * char_width);
        self.gl_forget_atlas();
        Ok(())
    }

    pub fn resize(&mut self, width: f64, height: f64, dpr: f64) {
        self.overview = None;
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

    /// The lines on screen as pairs of first and end line (exclusive), one pair per run of
    /// lines not hidden by a fold.
    #[wasm_bindgen(js_name = visibleLines)]
    pub fn visible_lines(&mut self) -> Vec<u32> {
        let model = self.model.borrow();
        self.view.sync(&model);
        let rope = model.rope();
        let mut lines = self.view.visible_lines(rope);
        let minimap = self.view.minimap_lines(rope);
        if !minimap.is_empty() {
            lines = lines.start.min(minimap.start)..lines.end.max(minimap.end);
        }
        self.view.shown_segments(lines).into_iter().flat_map(|r| [r.start as u32, r.end as u32]).collect()
    }

    /// Takes highlights from a language module: quads of line, start column, end column
    /// (chars) and style id, covering lines from `first_line`. Lines without quads are plain.
    /// Replaces all highlights; [`Editor::add_highlights`] adds another run of lines.
    #[wasm_bindgen(js_name = setHighlights)]
    pub fn set_highlights(&mut self, first_line: u32, end_line: u32, quads: &[u32]) {
        // Lines outside the new run are off screen; dropping them changes nothing drawn.
        self.highlights.sync(&self.model.borrow());
        let kept =
            self.highlights.lines.range(first_line as usize..end_line as usize).map(|(k, v)| (*k, v.clone())).collect();
        self.highlights.lines = kept;
        self.add_highlights(first_line, end_line, quads);
    }

    /// Adds highlights for another run of lines, as [`Editor::set_highlights`] takes them.
    #[wasm_bindgen(js_name = addHighlights)]
    pub fn add_highlights(&mut self, first_line: u32, end_line: u32, quads: &[u32]) {
        // The quads describe the text as it is now; semantic tokens move there first.
        self.highlights.sync(&self.model.borrow());
        let mut fresh: std::collections::BTreeMap<usize, Vec<StyleSpan>> =
            (first_line..end_line).map(|l| (l as usize, Vec::new())).collect();
        for q in quads.chunks_exact(4) {
            if let Some(spans) = fresh.get_mut(&(q[0] as usize)) {
                spans.push(StyleSpan { start: q[1] as usize, end: q[2] as usize, style: q[3] });
            }
        }
        let lines = &mut self.highlights.lines;
        for (line, spans) in fresh {
            // A line without spans and a missing line draw the same.
            let old = lines.get(&line).map_or(&[][..], Vec::as_slice);
            if old != spans.as_slice() {
                self.highlights.changed = true;
            }
            lines.insert(line, spans);
        }
    }

    /// Whether highlights or the bracket pair set since the last call differ from what was
    /// drawn; clears the flag. The JS side draws a frame for a worker reply only then.
    #[wasm_bindgen(js_name = takeHighlightsChanged)]
    pub fn take_highlights_changed(&mut self) -> bool {
        std::mem::take(&mut self.highlights.changed)
    }

    /// Whether the primary caret sits inside a string or comment, judged from the highlight
    /// styles around it. Used when the language module runs in a worker and cannot answer
    /// at once; auto-closing pairs stay closed inside literals.
    #[wasm_bindgen(js_name = caretInLiteral)]
    pub fn caret_in_literal(&mut self) -> bool {
        let model = self.model.borrow();
        self.cursors.sync(&model);
        self.highlights.sync(&model);
        let rope = model.rope();
        let head = self.cursors.selections().primary().head;
        let line = rope.char_to_line(head);
        let col = head - rope.line_to_char(line);
        let spans = self.highlights.spans(line);
        let kind_at = |c: usize| {
            let style = spans.iter().find(|s| s.start <= c && c < s.end).map(|s| s.style)?;
            let name = rustaco_syntax::name_of_style(style)?;
            if name.starts_with("comment") {
                Some(true)
            } else if name.starts_with("string") {
                Some(false)
            } else {
                None
            }
        };
        let before = col.checked_sub(1).and_then(kind_at);
        let at = kind_at(col);
        let at_line_end = col >= rustaco_core::line_len(rope, line);
        match (before, at) {
            (Some(_), Some(_)) => true,
            // A line comment runs to the end of its line.
            (Some(true), None) => at_line_end,
            _ => false,
        }
    }

    /// Recomputes the regions that can be folded, from indentation. The JS side calls it
    /// once typing pauses; with a syntax tree it passes ranges through
    /// [`Editor::set_folding_ranges`] instead.
    #[wasm_bindgen(js_name = refreshFoldingRanges)]
    pub fn refresh_folding_ranges(&mut self) {
        let model = self.model.borrow();
        self.view.sync(&model);
        self.view.set_foldable(indent_fold_ranges(model.rope(), model.options.tab_size));
    }

    /// [`Editor::set_folding_ranges`] for regions from the syntax tree: languages whose blocks
    /// open after a line ending in `:` (Python, YAML; `indent_after` in the catalog) also get
    /// the regions from indentation.
    #[wasm_bindgen(js_name = setTreeFoldingRanges)]
    pub fn set_tree_folding_ranges(&mut self, pairs: &[u32]) {
        let by_indentation = !self.model.borrow().editing.indent_after.is_empty();
        self.set_folding_ranges(pairs, by_indentation);
    }

    /// True for commands the frontend runs itself (they need the language module, providers
    /// or the DOM); `key_down` reports these instead of running them.
    #[wasm_bindgen(js_name = isFrontendCommand)]
    pub fn is_frontend_command(&self, command: &str) -> bool {
        FRONTEND_COMMANDS.contains(&command)
    }

    /// What a frame needs from the language module, once per change of text, visible lines,
    /// caret or bracket colouring: the caret offset followed by the visible runs of lines
    /// (pairs of first and end line). Empty when nothing changed since the last call, or
    /// since [`Editor::forget_highlight_request`].
    #[wasm_bindgen(js_name = highlightRequest)]
    pub fn highlight_request(&mut self, colorize: bool) -> Vec<u32> {
        let runs = self.visible_lines();
        let caret = self.primary_range()[0];
        let version = self.model.borrow().version();
        let key = (version, runs.clone(), caret, colorize);
        if self.highlight_key.as_ref() == Some(&key) {
            return Vec::new();
        }
        self.highlight_key = Some(key);
        std::iter::once(caret).chain(runs).collect()
    }

    /// Makes the next [`Editor::highlight_request`] ask again (a background parse finished).
    #[wasm_bindgen(js_name = forgetHighlightRequest)]
    pub fn forget_highlight_request(&mut self) {
        self.highlight_key = None;
    }

    /// Sets the regions that can be folded: pairs of header line and last line, from a syntax
    /// tree. With `with_indentation`, regions from indentation are added where no pair starts
    /// on the same line (Python, YAML: blocks without brackets).
    #[wasm_bindgen(js_name = setFoldingRanges)]
    pub fn set_folding_ranges(&mut self, pairs: &[u32], with_indentation: bool) {
        let model = self.model.borrow();
        self.view.sync(&model);
        let mut ranges: Vec<(usize, usize)> = pairs.chunks_exact(2).map(|p| (p[0] as usize, p[1] as usize)).collect();
        if with_indentation {
            let headers: std::collections::HashSet<usize> = ranges.iter().map(|r| r.0).collect();
            ranges.extend(
                indent_fold_ranges(model.rope(), model.options.tab_size)
                    .into_iter()
                    .filter(|r| !headers.contains(&r.0)),
            );
        }
        self.view.set_foldable(ranges);
    }

    /// Folds or unfolds the region headed at `line`, as a click on the gutter marker does.
    #[wasm_bindgen(js_name = toggleFold)]
    pub fn toggle_fold(&mut self, line: u32) -> bool {
        let model = self.model.borrow();
        self.view.toggle_fold(&model, line as usize)
    }

    /// Drops highlights, e.g. when the model's language is turned off.
    #[wasm_bindgen(js_name = clearHighlights)]
    pub fn clear_highlights(&mut self) {
        self.highlights.lines.clear();
    }

    /// Scrolls to the top; used after the model's text is replaced.
    pub fn scroll_to_top(&mut self) {
        self.view.scroll_to(self.model.borrow().rope(), 0.0, 0.0);
    }

    /// Vertical scroll position in CSS pixels.
    #[wasm_bindgen(js_name = scrollTop)]
    pub fn scroll_top_px(&self) -> f64 {
        self.view.scroll_top()
    }

    #[wasm_bindgen(js_name = setScrollTop)]
    pub fn set_scroll_top(&mut self, top: f64) {
        let model = self.model.borrow();
        self.view.sync(&model);
        let left = self.view.scroll_left();
        self.view.scroll_to(model.rope(), top, left);
    }

    pub fn line_height(&self) -> f64 {
        self.view.metrics.line_height as f64
    }

    /// Width of one cell in CSS pixels.
    #[wasm_bindgen(js_name = charWidth)]
    pub fn char_width(&self) -> f64 {
        self.view.metrics.char_width as f64
    }

    /// Primary caret position in CSS pixels, for placing the hidden textarea (IME popups).
    pub fn caret_x(&self) -> f64 {
        self.caret.x as f64
    }

    pub fn caret_y(&self) -> f64 {
        self.caret.y as f64
    }

    /// Selects the keymap: `preset` is `"default"` (VS Code) or `"jetbrains"`, `platform` is
    /// `"mac"`, `"windows"` or `"linux"`.
    #[wasm_bindgen(js_name = setKeymap)]
    pub fn set_keymap(&mut self, preset: &str, platform: &str) -> bool {
        self.platform = match platform {
            "mac" => Platform::Mac,
            "linux" => Platform::Linux,
            _ => Platform::Windows,
        };
        self.keymap = match preset {
            "default" => default_keymap(self.platform),
            "jetbrains" => jetbrains_keymap(self.platform),
            _ => return false,
        };
        true
    }

    /// Adds keybindings over the current keymap: newline-separated keys (`"ctrl+k ctrl+c"`),
    /// commands (`-command` removes a binding) and `when` clauses (empty for none).
    #[wasm_bindgen(js_name = addKeybindings)]
    pub fn add_keybindings(&mut self, keys: &str, commands: &str, whens: &str) {
        let entries: Vec<(&str, &str, Option<&str>)> = keys
            .split('\n')
            .zip(commands.split('\n'))
            .zip(whens.split('\n').chain(std::iter::repeat("")))
            .map(|((k, c), w)| (k, c, (!w.is_empty()).then_some(w)))
            .collect();
        self.keymap.extend(entries);
    }

    /// Runs a command by id (see the keymap). Returns false for unknown ids.
    #[wasm_bindgen(js_name = runCommand)]
    pub fn run_command(&mut self, command: &str) -> bool {
        let page = self.view.page_rows() as isize;
        let page_move = match command {
            "cursorPageUp" => Some((-page, false)),
            "cursorPageUpSelect" => Some((-page, true)),
            "cursorPageDown" => Some((page, false)),
            "cursorPageDownSelect" => Some((page, true)),
            _ => None,
        };
        let vertical = match command {
            "cursorUp" => Some((-1, false)),
            "cursorUpSelect" => Some((-1, true)),
            "cursorDown" => Some((1, false)),
            "cursorDownSelect" => Some((1, true)),
            _ => None,
        };
        if let Some((lines, extend)) = vertical {
            self.move_vertical(lines, extend);
            self.reveal_primary();
            return true;
        }
        if let Some(handled) = self.fold_command(command) {
            return handled;
        }
        let find_step = match command {
            "editor.action.nextMatchFindAction" => Some(true),
            "editor.action.jumpToBracket" => {
                let Some((open, close)) = self.current_bracket_pair() else { return false };
                let head = self.selections_primary().head;
                let target = if head == open || head == open + 1 { close } else { open };
                self.edit(|ed| ed.set_selections(Selections::single(Selection::cursor(target))));
                self.reveal_primary();
                return true;
            }
            "editor.action.smartSelect.shrink" => {
                let ok = self.edit(|ed| ed.shrink_selection());
                self.reveal_primary();
                return ok;
            }
            "editor.action.previousMatchFindAction" => Some(false),
            _ => None,
        };
        if let Some(forward) = find_step {
            return self.find_next(forward);
        }
        let handled_page = match page_move {
            Some((lines, extend)) => {
                self.move_vertical(lines, extend);
                let dy = lines as f64 * self.view.metrics.line_height as f64;
                self.view.scroll_by(self.model.borrow().rope(), 0.0, dy);
                true
            }
            None => self.edit(|ed| ed.run(command)),
        };
        let handled = handled_page;
        if !command.contains("SnippetPlaceholder") {
            self.edit(|ed| ed.check_snippet());
        }
        if handled {
            self.reveal_primary();
        }
        handled
    }

    /// Handles a keydown through the keymap: 0 when the key is not bound (printable text,
    /// AltGr characters arrive through [`Editor::insert_text`]), 1 when it was handled, 2 when
    /// it resolved to a command the JS side must run ([`Editor::take_frontend_command`]).
    pub fn key_down(&mut self, key: &str, ctrl: bool, shift: bool, alt: bool, meta: bool) -> u8 {
        let pressed = KeyPress { ctrl, shift, alt, meta, key: key.to_lowercase() };
        let context = {
            let model = self.model.borrow();
            self.cursors.sync(&model);
            let sels = self.cursors.selections();
            HashMap::from([
                ("editorHasSelection".to_owned(), sels.iter().any(|s| !s.is_empty())),
                ("editorHasMultipleSelections".to_owned(), sels.len() > 1),
                ("editorTextFocus".to_owned(), true),
                ("inSnippetMode".to_owned(), self.cursors.snippet_active()),
            ])
        };
        match self.keymap.resolve(&pressed, &context) {
            Resolved::Command(command)
                if FRONTEND_COMMANDS.contains(&command.as_str()) || self.host_commands.contains(&command) =>
            {
                self.frontend_command = Some(command);
                2
            }
            Resolved::Command(command) => {
                self.run_command(&command);
                1
            }
            Resolved::Pending => 1,
            Resolved::Unbound => 0,
        }
    }

    /// Display options: ruler columns, whitespace marks, indentation guides, highlighting of
    /// other occurrences of the word at the cursor.
    #[wasm_bindgen(js_name = setViewOptions)]
    pub fn set_view_options(&mut self, rulers: &[u32], render_whitespace: bool, guides: bool, occurrences: bool) {
        self.options = ViewOptions {
            rulers: rulers.iter().map(|&r| r as usize).collect(),
            render_whitespace,
            guides,
            occurrences,
        };
    }

    /// Adds a view zone (empty space for a DOM node) after line `after_line` (-1: above the
    /// first line), `height` pixels tall.
    #[wasm_bindgen(js_name = addViewZone)]
    pub fn add_view_zone(&mut self, id: u32, after_line: i32, height: f64) {
        let model = self.model.borrow();
        self.view.add_zone(&model, id, usize::try_from(after_line).ok(), height as f32);
    }

    #[wasm_bindgen(js_name = removeViewZone)]
    pub fn remove_view_zone(&mut self, id: u32) -> bool {
        let model = self.model.borrow();
        self.view.remove_zone(&model, id)
    }

    /// View zones drawn in the last frame: id, x, y, width and height for each.
    #[wasm_bindgen(js_name = viewZoneRects)]
    pub fn view_zone_rects(&self) -> Vec<f64> {
        self.zone_rects
            .iter()
            .flat_map(|(id, r)| [*id as f64, r.x as f64, r.y as f64, r.width as f64, r.height as f64])
            .collect()
    }

    /// Where char `offset` is on screen: x, y and line height, or empty when its line is
    /// not visible. Content widgets are placed with it.
    #[wasm_bindgen(js_name = positionOf)]
    pub fn position_of(&mut self, offset: u32) -> Vec<f64> {
        let model = self.model.borrow();
        self.view.sync(&model);
        match self.view.position_rect(model.rope(), offset as usize) {
            Some(r) => vec![r.x as f64, r.y as f64, r.height as f64],
            None => Vec::new(),
        }
    }

    /// Recomputes the overview ruler marks at the next frame. The JS side calls it once
    /// typing pauses and after decorations or matches change.
    #[wasm_bindgen(js_name = refreshOverview)]
    pub fn refresh_overview(&mut self) {
        self.overview = None;
    }

    /// Pins up to `max` headers of the enclosing scopes at the top; 0 turns it off.
    #[wasm_bindgen(js_name = setStickyScroll)]
    pub fn set_sticky_scroll(&mut self, max: u32) {
        self.view.set_sticky_scroll(max as usize);
    }

    /// Shows or hides the minimap.
    #[wasm_bindgen(js_name = setMinimap)]
    pub fn set_minimap(&mut self, on: bool) {
        self.view.set_minimap(if on { MINIMAP_WIDTH } else { 0.0 });
    }

    /// Turns word wrap at the editor's width on or off.
    #[wasm_bindgen(js_name = setWordWrap)]
    pub fn set_word_wrap(&mut self, on: bool) {
        self.view.set_wrap(on);
        let model = self.model.borrow();
        self.view.sync(&model);
        let (top, left) = (self.view.scroll_top(), self.view.scroll_left());
        self.view.scroll_to(model.rope(), top, left);
    }

    /// The bracket pair at the cursor from the language module's tree ([open, close], or empty
    /// for none). Without this call the editor matches brackets by scanning the text.
    #[wasm_bindgen(js_name = setTreeBrackets)]
    pub fn set_tree_brackets(&mut self, pair: &[u32]) {
        let next = Some((pair.len() == 2).then(|| (pair[0] as usize, pair[1] as usize)));
        if next != self.tree_brackets {
            self.highlights.changed = true;
        }
        self.tree_brackets = next;
    }

    #[wasm_bindgen(js_name = clearTreeBrackets)]
    pub fn clear_tree_brackets(&mut self) {
        if self.tree_brackets.is_some() {
            self.highlights.changed = true;
        }
        self.tree_brackets = None;
    }

    /// Makes keybindings that resolve to `id` hand it to the JS side, where the embedder's
    /// handler runs (see `addCommand` in rustaco.js).
    #[wasm_bindgen(js_name = registerHostCommand)]
    pub fn register_host_command(&mut self, id: &str) {
        self.host_commands.insert(id.to_owned());
    }

    /// The command [`Editor::key_down`] left for the JS side, once.
    #[wasm_bindgen(js_name = takeFrontendCommand)]
    pub fn take_frontend_command(&mut self) -> String {
        self.frontend_command.take().unwrap_or_default()
    }

    /// Expands the primary selection to the next enclosing syntax node. `flat` holds node
    /// ranges from the language module as (start, end) pairs, innermost first.
    #[wasm_bindgen(js_name = expandSelection)]
    pub fn expand_selection(&mut self, flat: &[u32]) -> bool {
        let ranges: Vec<std::ops::Range<usize>> = flat.chunks_exact(2).map(|p| p[0] as usize..p[1] as usize).collect();
        let ok = self.edit(|ed| ed.expand_selection(&ranges));
        if ok {
            self.reveal_primary();
        }
        ok
    }

    /// The primary selection as [start, end] char offsets.
    #[wasm_bindgen(js_name = primaryRange)]
    pub fn primary_range(&mut self) -> Vec<u32> {
        let p = self.selections_primary();
        vec![p.start() as u32, p.end() as u32]
    }

    /// Searches the model and highlights every match. Returns the number of matches, or -1
    /// when the regular expression does not compile.
    pub fn find(&mut self, pattern: &str, regex: bool, case_sensitive: bool, whole_word: bool) -> i32 {
        let query = SearchQuery { pattern: pattern.to_owned(), regex, case_sensitive, whole_word };
        match Searcher::new(&query) {
            Ok(searcher) => {
                self.find = Some((query, searcher));
                self.refresh_find() as i32
            }
            Err(_) => {
                self.clear_find();
                if pattern.is_empty() { 0 } else { -1 }
            }
        }
    }

    /// Sets the query without searching, for when the matches come from the worker
    /// ([`Editor::set_find_matches`]). Returns 0, or -1 when the regular expression does
    /// not compile.
    #[wasm_bindgen(js_name = setFindQuery)]
    pub fn set_find_query(&mut self, pattern: &str, regex: bool, case_sensitive: bool, whole_word: bool) -> i32 {
        let query = SearchQuery { pattern: pattern.to_owned(), regex, case_sensitive, whole_word };
        match Searcher::new(&query) {
            Ok(searcher) if !pattern.is_empty() => {
                self.find = Some((query, searcher));
                0
            }
            Ok(_) => {
                self.clear_find();
                0
            }
            Err(_) => {
                self.clear_find();
                -1
            }
        }
    }

    /// Shows matches found elsewhere (pairs of start and end char offsets) for the current
    /// query; they move with edits like any decoration.
    #[wasm_bindgen(js_name = setFindMatches)]
    pub fn set_find_matches(&mut self, pairs: &[u32]) -> u32 {
        self.overview = None;
        let owner = self.cursors.id().0;
        let mut model = self.model.borrow_mut();
        if self.find.is_none() {
            model.decorations.clear(owner);
            return 0;
        }
        let len = model.len_chars();
        let ranges: Vec<std::ops::Range<usize>> =
            pairs.chunks_exact(2).map(|p| (p[0] as usize).min(len)..(p[1] as usize).min(len)).collect();
        let count = ranges.len() as u32;
        model.decorations.set(owner, FIND_MATCH, Stickiness::NeverGrows, ranges);
        count
    }

    /// Re-runs the current query, after the text changed. Returns the number of matches.
    #[wasm_bindgen(js_name = refreshFind)]
    pub fn refresh_find(&mut self) -> u32 {
        self.overview = None;
        let owner = self.cursors.id().0;
        let mut model = self.model.borrow_mut();
        let Some((_, searcher)) = &self.find else {
            model.decorations.clear(owner);
            return 0;
        };
        let matches = searcher.find_all(model.rope(), None);
        let count = matches.len() as u32;
        model.decorations.set(owner, FIND_MATCH, Stickiness::NeverGrows, matches);
        count
    }

    #[wasm_bindgen(js_name = clearFind)]
    pub fn clear_find(&mut self) {
        self.overview = None;
        self.find = None;
        let owner = self.cursors.id().0;
        self.model.borrow_mut().decorations.clear(owner);
    }

    /// Selects the next (or previous) match after the primary selection, wrapping around.
    #[wasm_bindgen(js_name = findNext)]
    pub fn find_next(&mut self, forward: bool) -> bool {
        let owner = self.cursors.id().0;
        let matches: Vec<std::ops::Range<usize>> = {
            let model = self.model.borrow();
            model.decorations.of_owner(owner).iter().map(|d| d.range.clone()).filter(|r| !r.is_empty()).collect()
        };
        let primary = self.selections_primary();
        let from = if forward { primary.end() } else { primary.start() };
        let Some(m) = next_match(&matches, from, forward) else { return false };
        self.edit(|ed| ed.set_selections(Selections::single(Selection::new(m.start, m.end))));
        self.reveal_primary();
        true
    }

    /// Replaces the selected match and moves to the next one.
    #[wasm_bindgen(js_name = replaceOne)]
    pub fn replace_one(&mut self, replacement: &str) -> bool {
        let primary = self.selections_primary();
        let Some((query, searcher)) = &self.find else { return false };
        let regex = query.regex;
        let is_match = {
            let model = self.model.borrow();
            searcher.find_all(model.rope(), Some(primary.start()..primary.end())).first()
                == Some(&(primary.start()..primary.end()))
        };
        if is_match {
            let text = searcher.replacement_for(
                self.model.borrow().rope(),
                primary.start()..primary.end(),
                replacement,
                regex,
            );
            self.edit(|ed| ed.insert(&text));
            self.refresh_find();
        }
        self.find_next(true)
    }

    /// Replaces every match as one undo step. Returns the number of replacements.
    #[wasm_bindgen(js_name = replaceAll)]
    pub fn replace_all(&mut self, replacement: &str) -> u32 {
        let Some((query, searcher)) = self.find.take() else { return 0 };
        let count = self.edit(|ed| ed.replace_all(&searcher, replacement, query.regex));
        self.find = Some((query, searcher));
        self.refresh_find();
        count as u32
    }

    /// Which match the primary selection is (1-based, 0 when it is none) and how many
    /// there are, for "3 of 12".
    #[wasm_bindgen(js_name = findPosition)]
    pub fn find_position(&mut self) -> Vec<u32> {
        let owner = self.cursors.id().0;
        let primary = self.selections_primary();
        let model = self.model.borrow();
        let matches: Vec<std::ops::Range<usize>> = model
            .decorations
            .of_owner(owner)
            .iter()
            .filter(|d| d.kind == FIND_MATCH)
            .map(|d| d.range.clone())
            .collect();
        let index =
            matches.iter().position(|m| m.start == primary.start() && m.end == primary.end()).map_or(0, |i| i + 1);
        vec![index as u32, matches.len() as u32]
    }

    /// The single-line text of the primary selection, to seed the find widget; empty
    /// when the selection is empty or spans lines.
    #[wasm_bindgen(js_name = selectionSeed)]
    pub fn selection_seed(&mut self) -> String {
        let primary = self.selections_primary();
        let model = self.model.borrow();
        let text: String = model.rope().slice(primary.start()..primary.end()).to_string();
        if text.contains('\n') || text.contains('\r') { String::new() } else { text }
    }

    /// Selects every match (one cursor per match).
    #[wasm_bindgen(js_name = selectAllMatches)]
    pub fn select_all_matches(&mut self) -> u32 {
        let Some((query, searcher)) = self.find.take() else { return 0 };
        let count = self.edit(|ed| ed.select_matches(&searcher));
        self.find = Some((query, searcher));
        count as u32
    }

    /// True while the first key of a chord (`ctrl+k`) waits for the second.
    #[wasm_bindgen(js_name = hasPendingChord)]
    pub fn has_pending_chord(&self) -> bool {
        self.keymap.has_pending_chord()
    }

    /// Typed text from the input element. A single char goes through bracket handling
    /// (auto-closing pairs, typing over a closing bracket), except that `in_literal` (the
    /// cursor is inside a string or comment) opens no pairs; longer text (IME, dictation) is
    /// inserted as is.
    pub fn insert_text(&mut self, text: &str, in_literal: bool) {
        let mut chars = text.chars();
        match (chars.next(), chars.next()) {
            (None, _) => return,
            (Some(c), None) => self.edit(|ed| ed.type_char_in(c, in_literal)),
            _ => self.edit(|ed| ed.insert(text)),
        }
        self.reveal_primary();
    }

    pub fn copy(&mut self) -> String {
        self.edit(|ed| ed.copy())
    }

    /// One step of an IME composition: replaces the `before` chars ahead of each caret (the
    /// previous step's text, 0 at the start) with `text` and underlines it.
    pub fn compose(&mut self, before: u32, text: &str) {
        self.edit(|ed| ed.compose(before as usize, text));
        let owner = COMPOSITION_OWNER + self.cursors.id().0;
        let len = text.chars().count();
        let mut model = self.model.borrow_mut();
        self.cursors.sync(&model);
        let ranges: Vec<std::ops::Range<usize>> =
            self.cursors.selections().iter().map(|s| s.head.saturating_sub(len)..s.head).collect();
        model.decorations.set(owner, COMPOSITION, Stickiness::NeverGrows, ranges);
        drop(model);
        self.reveal_primary();
    }

    /// Inserts a snippet (VS Code syntax) at every selection and selects its first tab stop.
    #[wasm_bindgen(js_name = insertSnippet)]
    pub fn insert_snippet(&mut self, source: &str) {
        let snippet = self.parse_snippet(source);
        self.edit(|ed| ed.insert_snippet(&snippet));
        self.reveal_primary();
    }

    /// Sets the items of the completion list, for [`Editor::rank_completions`]: per item the
    /// text it is filtered by (filter text or label) and its sort text, NUL-separated.
    #[wasm_bindgen(js_name = setCompletionItems)]
    pub fn set_completion_items(&mut self, texts: &str, sort_texts: &str, labels: &str) {
        let split = |s: &str| if s.is_empty() { Vec::new() } else { s.split('\0').map(str::to_owned).collect() };
        self.completion_items = (split(texts), split(sort_texts));
        self.completion_labels = split(labels);
    }

    /// The completion list after a keystroke: the text from `start` (the word start) to the
    /// caret filters and ranks the items. Answers `[0]` when the list should close (the caret
    /// left the word, a non-word char was typed, nothing matches, or without `explicit` the
    /// only match is already typed in full). Otherwise 1, the number of matches, then for at
    /// most `limit` of them: item index, number of matched chars and their indices in the
    /// label (none when the item filters by a text other than its label). With `keep_order`
    /// (a snippet's choices) nothing is typed yet and the items keep their order.
    #[wasm_bindgen(js_name = refilterCompletions)]
    pub fn refilter_completions(&mut self, start: u32, keep_order: bool, explicit: bool, limit: u32) -> Vec<u32> {
        let head = self.selections_primary().head;
        let start = start as usize;
        if head < start {
            return vec![0];
        }
        let typed = if keep_order {
            String::new()
        } else {
            let model = self.model.borrow();
            let len = model.len_chars();
            model.rope().slice(start.min(len)..head.min(len)).to_string()
        };
        if typed.chars().any(|c| !(c.is_alphanumeric() || c == '_')) {
            return vec![0];
        }
        let (texts, sorts) = &self.completion_items;
        let labels = if self.completion_labels.len() == texts.len() { &self.completion_labels } else { texts };
        let sorts = if sorts.len() == texts.len() { sorts } else { texts };
        let ranked = rustaco_core::rank_completions(&typed, texts, sorts, keep_order);
        let only_typed = ranked.len() == 1 && labels[ranked[0].0] == typed;
        if ranked.is_empty() || (!explicit && only_typed) {
            return vec![0];
        }
        let mut out = vec![1, ranked.len() as u32];
        for (i, m) in ranked.into_iter().take(limit as usize) {
            out.push(i as u32);
            if texts[i] == labels[i] {
                out.push(m.matches.len() as u32);
                out.extend(m.matches.iter().map(|&c| c as u32));
            } else {
                out.push(0);
            }
        }
        out
    }

    /// [`Editor::accept_completion`] for an item that replaces `start..end`: the caret's
    /// distance to both ends gives what to replace at every cursor.
    #[wasm_bindgen(js_name = acceptCompletionAt)]
    pub fn accept_completion_at(&mut self, start: u32, end: u32, text: &str, snippet: bool) {
        let head = self.selections_primary().head as u32;
        self.accept_completion(head.saturating_sub(start), end.saturating_sub(head), text, snippet);
    }

    /// Filters and ranks the completion items by what was typed, best first (in the given
    /// order with `keep_order`, for snippet choices). The number of matches, then for at most
    /// `limit` of them: item index, number of matched chars, then their char indices.
    #[wasm_bindgen(js_name = rankCompletions)]
    pub fn rank_completions(&self, typed: &str, keep_order: bool, limit: u32) -> Vec<u32> {
        let (texts, sorts) = &self.completion_items;
        let sorts = if sorts.len() == texts.len() { sorts } else { texts };
        let ranked = rustaco_core::rank_completions(typed, texts, sorts, keep_order);
        let mut out = vec![ranked.len() as u32];
        for (i, m) in ranked.into_iter().take(limit as usize) {
            out.push(i as u32);
            out.push(m.matches.len() as u32);
            out.extend(m.matches.iter().map(|&c| c as u32));
        }
        out
    }

    /// Makes the words around `line` the completion items (for languages without a
    /// completion provider), without `exclude`; they stay in Rust, and the list asks for the
    /// texts of the few it shows with [`Editor::completion_texts`]. Returns their number.
    #[wasm_bindgen(js_name = setWordCompletions)]
    pub fn set_word_completions(&mut self, model: &Model, line: u32, radius: u32, max: u32, exclude: &str) -> u32 {
        let words = model.words_near(line as usize, radius as usize, max as usize, exclude);
        let n = words.len() as u32;
        self.completion_items = (words, Vec::new());
        self.completion_labels = Vec::new();
        n
    }

    /// The filter texts of the completion items at `indices`, NUL-separated.
    #[wasm_bindgen(js_name = completionTexts)]
    pub fn completion_texts(&self, indices: &[u32]) -> String {
        let texts = &self.completion_items.0;
        indices.iter().filter_map(|&i| texts.get(i as usize).map(String::as_str)).collect::<Vec<_>>().join("\0")
    }

    /// Accepts a completion at every cursor: replaces `before` chars ahead of each caret and
    /// `after` chars behind it with `text`, as a snippet when `snippet` is true.
    #[wasm_bindgen(js_name = acceptCompletion)]
    pub fn accept_completion(&mut self, before: u32, after: u32, text: &str, snippet: bool) {
        let parsed = snippet.then(|| self.parse_snippet(text));
        self.edit(|ed| {
            let len = ed.model().len_chars();
            let ranges: Vec<Selection> = ed
                .selections()
                .iter()
                .map(|s| Selection::new(s.head.saturating_sub(before as usize), (s.head + after as usize).min(len)))
                .collect();
            let primary = ed.selections().primary_index();
            ed.set_selections(Selections::new(ranges, primary));
            match &parsed {
                Some(sn) => ed.insert_snippet(sn),
                None => ed.insert(text),
            }
        });
        self.reveal_primary();
    }

    /// The options of the snippet stop being edited, when it is a choice.
    #[wasm_bindgen(js_name = snippetChoices)]
    pub fn snippet_choices(&mut self) -> Vec<String> {
        self.edit(|ed| ed.snippet().and_then(|s| s.choices()).map(<[String]>::to_vec).unwrap_or_default())
    }

    /// The char offset under a point in canvas coordinates, or -1 when the point is not over
    /// text (the gutter, past a line's end, the minimap).
    #[wasm_bindgen(js_name = textOffsetAt)]
    pub fn text_offset_at(&mut self, x: f64, y: f64) -> i32 {
        let model = self.model.borrow();
        self.view.sync(&model);
        let rope = model.rope();
        if x < self.view.gutter_width(rope) as f64 || self.view.in_minimap(x as f32) {
            return -1;
        }
        let pos = self.view.hit_test(rope, x as f32, y as f32);
        let line = rope.char_to_line(pos);
        let at_end = pos == rustaco_core::line_end(rope, line);
        if at_end {
            // Past the last char: only a hit when the point is still on that char's cell.
            match self.view.position_rect(rope, pos) {
                Some(r) if (x as f32) < r.x => pos.saturating_sub(1) as i32,
                _ => -1,
            }
        } else {
            pos as i32
        }
    }

    /// Applies edits as one undo step: `ranges` holds start and end char offsets per edit,
    /// `texts` the replacement texts. Edits must not overlap.
    #[wasm_bindgen(js_name = applyEdits)]
    pub fn apply_edits_js(&mut self, ranges: &[u32], texts: Vec<String>) {
        let edits: Vec<rustaco_core::Edit> = ranges
            .chunks_exact(2)
            .zip(texts)
            .map(|(r, t)| rustaco_core::Edit::new(r[0] as usize, r[1] as usize, t))
            .collect();
        self.edit(|ed| ed.apply_edits(edits));
    }

    /// X of the first non-blank char of `line`, from the text area's left edge, for placing
    /// widgets above a line's code.
    #[wasm_bindgen(js_name = indentOfLine)]
    pub fn indent_of_line(&mut self, line: u32) -> f64 {
        let model = self.model.borrow();
        let rope = model.rope();
        let line = (line as usize).min(rope.len_lines() - 1);
        let tab = model.options.tab_size.max(1);
        let cells = rope
            .line(line)
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .fold(0, |acc, c| if c == '\t' { acc + tab - acc % tab } else { acc + 1 });
        (cells as f32 * self.view.metrics.char_width) as f64
    }

    /// Shows inlay hints: `labels[i]` before the char at `offsets[i]`.
    #[wasm_bindgen(js_name = setInlayHints)]
    pub fn set_inlay_hints(&mut self, offsets: &[u32], labels: Vec<String>) {
        let model = self.model.borrow();
        let hints = offsets.iter().zip(labels).map(|(&o, l)| (o as usize, l)).collect();
        self.view.set_inlays(&model, hints);
    }

    /// [`Editor::set_semantic_tokens`] with style names: `ranges` holds start and end per
    /// token, `styles` the highlight names joined by newlines (`"function\nvariable"`).
    #[wasm_bindgen(js_name = setSemanticTokensNamed)]
    pub fn set_semantic_tokens_named(&mut self, ranges: &[u32], styles: &str) {
        let mut triples: Vec<[u32; 3]> =
            ranges.chunks_exact(2).zip(styles.split('\n')).map(|(r, s)| [r[0], r[1], style_id(s)]).collect();
        triples.sort_unstable_by_key(|t| t[0]);
        self.set_semantic_tokens(&triples.concat());
    }

    /// Takes semantic tokens: triples of start and end char offsets and style id
    /// ([`style_id`]), drawn over the syntax highlights until replaced.
    #[wasm_bindgen(js_name = setSemanticTokens)]
    pub fn set_semantic_tokens(&mut self, triples: &[u32]) {
        let model = self.model.borrow();
        let rope = model.rope();
        let len = rope.len_chars();
        let mut semantic: std::collections::BTreeMap<usize, Vec<StyleSpan>> = Default::default();
        for t in triples.chunks_exact(3) {
            let (start, end) = ((t[0] as usize).min(len), (t[1] as usize).min(len));
            if start >= end {
                continue;
            }
            // A token stays on its first line.
            let line = rope.char_to_line(start);
            let line_start = rope.line_to_char(line);
            let line_end = line_start + rustaco_core::line_len(rope, line);
            semantic.entry(line).or_default().push(StyleSpan {
                start: start - line_start,
                end: end.min(line_end) - line_start,
                style: t[2],
            });
        }
        for spans in semantic.values_mut() {
            spans.sort_by_key(|s| s.start);
            spans.dedup_by(|b, a| b.start < a.end);
        }
        // The syntax spans move to the current text first; the new tokens describe it.
        self.highlights.sync(&model);
        self.highlights.semantic = semantic;
    }

    /// The word at the primary caret: [start, end] char offsets (equal when there is none).
    #[wasm_bindgen(js_name = wordAtCaret)]
    pub fn word_at_caret(&mut self) -> Vec<u32> {
        let head = self.selections_primary().head;
        let model = self.model.borrow();
        let (a, b) = word_at(model.rope(), head);
        let is_word = model.rope().slice(a..b).chars().all(|c| c.is_alphanumeric() || c == '_');
        if is_word && a < b { vec![a as u32, b as u32] } else { vec![head as u32, head as u32] }
    }

    /// Ends the composition: its text stays, the underline goes.
    #[wasm_bindgen(js_name = endComposition)]
    pub fn end_composition(&mut self) {
        self.model.borrow_mut().decorations.clear(COMPOSITION_OWNER + self.cursors.id().0);
    }

    /// The text of each non-empty selection, for clipboard metadata.
    #[wasm_bindgen(js_name = selectionTexts)]
    pub fn selection_texts(&mut self) -> Vec<String> {
        self.edit(|ed| ed.selection_texts())
    }

    /// Pastes one piece per cursor (as copied from that many selections), or the pieces
    /// joined when the counts differ.
    #[wasm_bindgen(js_name = pasteEach)]
    pub fn paste_each(&mut self, pieces: Vec<String>) {
        self.edit(|ed| ed.paste_each(&pieces));
        self.reveal_primary();
    }

    /// The primary caret's line for screen readers, mirrored into the hidden textarea: the
    /// line's text (at most 2000 chars around the caret) and the selection in it as UTF-16
    /// offsets, start then end (the selection is clipped to the line).
    #[wasm_bindgen(js_name = accessibleLine)]
    pub fn accessible_line(&mut self) -> js_sys::Array {
        let model = self.model.borrow();
        self.cursors.sync(&model);
        let rope = model.rope();
        let sel = self.cursors.selections().primary();
        let line = rope.char_to_line(sel.head);
        let start = rope.line_to_char(line);
        let len = rustaco_core::line_len(rope, line);
        let head = sel.head - start;
        let from = head.saturating_sub(1000);
        let to = (head + 1000).min(len);
        let text: String = rope.slice(start + from..start + to).to_string();
        let utf16 = |col: usize| -> u32 {
            let col = col.clamp(from, to);
            rope.slice(start + from..start + col).chars().map(char::len_utf16).sum::<usize>() as u32
        };
        let (a, b) = (sel.start().max(start) - start, sel.end().min(start + len).max(start) - start);
        let out = js_sys::Array::new();
        out.push(&JsValue::from_str(&text));
        out.push(&JsValue::from(utf16(a)));
        out.push(&JsValue::from(utf16(b)));
        out
    }

    /// The selections as HTML with syntax colours, for copying into rich text editors.
    /// Colours come from the highlights of the lines on screen; text beyond them is plain.
    #[wasm_bindgen(js_name = copyHtml)]
    pub fn copy_html(&mut self) -> String {
        let model = self.model.borrow();
        self.cursors.sync(&model);
        let rope = model.rope();
        let mut by_line: HashMap<usize, Vec<(usize, usize, u32)>> = HashMap::new();
        for sel in self.cursors.selections().iter() {
            for line in rope.char_to_line(sel.start())..=rope.char_to_line(sel.end()) {
                by_line
                    .entry(line)
                    .or_insert_with(|| self.highlights.spans(line).iter().map(|s| (s.start, s.end, s.style)).collect());
            }
        }
        let chrome = CHROME.with(|c| c.borrow().clone());
        let styles = STYLES.with(|s| s.borrow().clone());
        let escape = |c: char, out: &mut String| match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            _ => out.push(c),
        };
        let mut html = format!(
            "<pre style=\"background:{};color:{};font-family:{}\">",
            &chrome.background[..7],
            &styles[0].color[..7],
            self.font.split_once(' ').map_or("monospace", |(_, family)| family).replace('"', "'")
        );
        for (i, sel) in self.cursors.selections().iter().filter(|s| !s.is_empty()).enumerate() {
            if i > 0 {
                html.push('\n');
            }
            let mut pos = sel.start();
            while pos < sel.end() {
                let line = rope.char_to_line(pos);
                let line_start = rope.line_to_char(line);
                let col = pos - line_start;
                let spans = by_line.get(&line);
                let span = spans.and_then(|v| v.iter().find(|&&(a, b, _)| a <= col && col < b));
                let end = match span {
                    Some(&(_, b, _)) => (line_start + b).min(sel.end()),
                    None => {
                        let next = spans.and_then(|v| v.iter().filter(|&&(a, _, _)| a > col).map(|&(a, _, _)| a).min());
                        let line_end = rope.line_to_char((line + 1).min(rope.len_lines()));
                        next.map_or(line_end, |a| line_start + a).min(sel.end()).max(pos + 1)
                    }
                };
                let text: String = rope.slice(pos..end).chars().collect();
                match span.and_then(|&(_, _, style)| styles.get(style as usize)) {
                    Some(style) => {
                        html.push_str(&format!("<span style=\"color:{}\">", &style.color[..7]));
                        text.chars().for_each(|c| escape(c, &mut html));
                        html.push_str("</span>");
                    }
                    None => text.chars().for_each(|c| escape(c, &mut html)),
                }
                pos = end;
            }
        }
        html.push_str("</pre>");
        html
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

    /// What a copy or cut puts on the clipboard next to the text so a paste can go back one
    /// piece per cursor: JSON `{ pieces }` when there are several selections, else `""`.
    #[wasm_bindgen(js_name = clipboardMeta)]
    pub fn clipboard_meta(&mut self) -> String {
        let pieces = self.selection_texts();
        if pieces.len() > 1 { serde_json::json!({ "pieces": pieces }).to_string() } else { String::new() }
    }

    /// Pastes clipboard `text`. With `meta` from [`Editor::clipboard_meta`] whose pieces match
    /// the text and the cursor count, each cursor gets its piece; otherwise every cursor gets
    /// the whole text.
    #[wasm_bindgen(js_name = pasteClipboard)]
    pub fn paste_clipboard(&mut self, text: &str, meta: &str) {
        let pieces: Vec<String> = serde_json::from_str::<serde_json::Value>(meta)
            .ok()
            .and_then(|v| {
                v["pieces"].as_array().map(|a| a.iter().filter_map(|p| p.as_str().map(str::to_owned)).collect())
            })
            .unwrap_or_default();
        let cursors = self.selections().len() / 2;
        let same_text = pieces.join("\n") == text.replace("\r\n", "\n");
        if pieces.len() > 1 && pieces.len() == cursors && same_text {
            self.paste_each(pieces);
        } else {
            self.paste(text);
        }
    }

    /// A wheel event: `mode` is the DOM's `deltaMode` (0 pixels, 1 lines, 2 pages). With
    /// Shift and no horizontal delta, the vertical delta scrolls sideways.
    #[wasm_bindgen(js_name = wheelEvent)]
    pub fn wheel_event(&mut self, dx: f64, dy: f64, mode: u32, shift: bool) {
        let unit = match mode {
            1 => self.view.metrics.line_height as f64,
            2 => self.view.height as f64,
            _ => 1.0,
        };
        let (dx, dy) = if shift && dx == 0.0 { (dy, 0.0) } else { (dx, dy) };
        self.wheel(dx * unit, dy * unit);
    }

    /// One step of scrolling while a drag selection is held past the top or bottom edge,
    /// faster the further out the pointer is (at most 100 pixels a step); the selection
    /// follows the pointer. False when the pointer is back inside, so the timer can stop.
    #[wasm_bindgen(js_name = dragAutoscroll)]
    pub fn drag_autoscroll(&mut self, x: f64, y: f64) -> bool {
        let h = self.view.height as f64;
        let over = if y < 0.0 {
            y
        } else if y > h {
            y - h
        } else {
            0.0
        };
        if over == 0.0 {
            return false;
        }
        self.wheel(0.0, over.signum() * over.abs().min(200.0) / 2.0);
        self.mouse_move(x, y);
        true
    }

    /// [`Editor::key_down`] for a DOM key event. With Shift a symbol key types another char
    /// (`|` for `\` on US layouts), so it is named by its physical `code` instead, as VS Code
    /// does; otherwise the produced char counts, which keeps bindings working on other layouts.
    #[wasm_bindgen(js_name = keyDownEvent)]
    pub fn key_down_event(&mut self, key: &str, code: &str, ctrl: bool, shift: bool, alt: bool, meta: bool) -> u8 {
        let symbol = match code {
            "Backslash" => Some("\\"),
            "Slash" => Some("/"),
            "BracketLeft" => Some("["),
            "BracketRight" => Some("]"),
            "Semicolon" => Some(";"),
            "Quote" => Some("'"),
            "Comma" => Some(","),
            "Period" => Some("."),
            "Minus" => Some("-"),
            "Equal" => Some("="),
            "Backquote" => Some("`"),
            _ => None,
        };
        let key = match symbol {
            Some(s) if shift => s,
            _ => key,
        };
        self.key_down(key, ctrl, shift, alt, meta)
    }

    pub fn wheel(&mut self, dx: f64, dy: f64) {
        self.view.scroll_by(self.model.borrow().rope(), dx as f32, dy);
    }

    /// `clicks` is the DOM `detail`: 1 places a cursor, 2 selects a word, 3 a line.
    pub fn mouse_down(&mut self, x: f64, y: f64, clicks: u32, shift: bool, alt: bool) {
        let unit = match clicks {
            0 | 1 => DragUnit::Char,
            2 => DragUnit::Word,
            _ => DragUnit::Line,
        };
        if self.scrollbar_down(x, y) {
            return;
        }
        // A press inside a selection may start dragging the selected text.
        if clicks <= 1 && !shift && !alt {
            let model = self.model.borrow();
            self.view.sync(&model);
            self.cursors.sync(&model);
            let rope = model.rope();
            let gutter = self.view.gutter_width(rope) as f64;
            let pos = self.view.hit_test(rope, x as f32, y as f32);
            let inside = self.cursors.selections().iter().find(|s| !s.is_empty() && s.start() < pos && pos < s.end());
            if x > gutter
                && !self.view.in_minimap(x as f32)
                && let Some(sel) = inside
            {
                self.text_drag = Some((sel.start()..sel.end(), None, x, y));
                return;
            }
        }
        if let Some(index) =
            self.sticky.iter().position(|&(top, height, _)| (top as f64..(top + height) as f64).contains(&y))
        {
            let line = self.sticky[index].2;
            let (pos, top) = {
                let model = self.model.borrow();
                let rope = model.rope();
                let indent = rope.line(line).chars().take_while(|c| *c == ' ' || *c == '\t').count();
                let lh = self.view.metrics.line_height as f64;
                (rope.line_to_char(line) + indent, self.view.line_top(line) - index as f64 * lh)
            };
            self.edit(|ed| ed.set_selections(Selections::single(Selection::cursor(pos))));
            let model = self.model.borrow();
            let left = self.view.scroll_left();
            self.view.scroll_to(model.rope(), top, left);
            return;
        }
        if self.view.in_minimap(x as f32) {
            let model = self.model.borrow();
            self.view.sync(&model);
            let rope = model.rope();
            let slider = self.view.minimap_slider(rope);
            if !(slider.y as f64..(slider.y + slider.height) as f64).contains(&y) {
                let top = self.view.minimap_scroll_target(rope, y as f32);
                self.view.scroll_to(rope, top, self.view.scroll_left());
            }
            self.minimap_drag = Some((y, self.view.scroll_top()));
            return;
        }
        {
            let model = self.model.borrow();
            self.view.sync(&model);
            let rope = model.rope();
            if self.view.in_fold_gutter(rope, x as f32) {
                let (line, _) = self.view.line_and_cells_at(rope, x as f32, y as f32);
                if self.view.toggle_fold(&model, line) {
                    return;
                }
            }
        }
        let (pos, (start, end)) = {
            let model = self.model.borrow();
            let pos = self.view.hit_test(model.rope(), x as f32, y as f32);
            (pos, unit_range(model.rope(), unit, pos))
        };
        if shift && alt && clicks <= 1 {
            let anchor = self.view.line_and_cells_at(self.model.borrow().rope(), x as f32, y as f32);
            self.drag = Some(Drag { start: pos, end: pos, unit, add: false, column: Some(anchor) });
            self.edit(|ed| ed.set_selections(Selections::single(Selection::cursor(pos))));
            return;
        }
        self.drag = Some(self.edit(|ed| {
            let sels = ed.selections();
            if shift && clicks <= 1 {
                let anchor = sels.primary().anchor;
                ed.set_selections(Selections::single(Selection::new(anchor, pos)));
                Drag { start: anchor, end: anchor, unit, add: false, column: None }
            } else if alt {
                let mut ranges = sels.as_slice().to_vec();
                ranges.push(Selection::new(start, end));
                let primary = ranges.len() - 1;
                ed.set_selections(Selections::new(ranges, primary));
                Drag { start, end, unit, add: true, column: None }
            } else {
                ed.set_selections(Selections::single(Selection::new(start, end)));
                Drag { start, end, unit, add: false, column: None }
            }
        }));
    }

    /// Extends the selection while a button is held. Returns false when no drag is active.
    pub fn mouse_move(&mut self, x: f64, y: f64) -> bool {
        if let Some((vertical, start, start_scroll, factor)) = self.scrollbar_drag {
            let model = self.model.borrow();
            let rope = model.rope();
            let moved = (if vertical { y } else { x }) - start;
            if vertical {
                self.view.scroll_to(rope, start_scroll + moved * factor, self.view.scroll_left());
            } else {
                let top = self.view.scroll_top();
                self.view.scroll_to(rope, top, (start_scroll + moved * factor) as f32);
            }
            return true;
        }
        if let Some((range, _, x0, y0)) = self.text_drag.clone() {
            if (x - x0).abs() + (y - y0).abs() < 4.0 {
                return false;
            }
            let model = self.model.borrow();
            let pos = self.view.hit_test(model.rope(), x as f32, y as f32);
            self.text_drag = Some((range, Some(pos), x0, y0));
            return true;
        }
        if let Some((start_y, start_top)) = self.minimap_drag {
            let model = self.model.borrow();
            let rope = model.rope();
            let top = start_top + (y - start_y) * self.view.minimap_drag_factor(rope);
            self.view.scroll_to(rope, top, self.view.scroll_left());
            return true;
        }
        let Some(drag) = self.drag else { return false };
        if let Some((anchor_line, anchor_cells)) = drag.column {
            let selections = {
                let model = self.model.borrow();
                let rope = model.rope();
                let (head_line, head_cells) = self.view.line_and_cells_at(rope, x as f32, y as f32);
                let lines: Vec<usize> = if head_line >= anchor_line {
                    (anchor_line..=head_line).collect()
                } else {
                    (head_line..=anchor_line).rev().collect()
                };
                let ranges: Vec<Selection> = lines
                    .iter()
                    .map(|&l| {
                        Selection::new(
                            self.view.offset_at_cells(rope, l, anchor_cells),
                            self.view.offset_at_cells(rope, l, head_cells),
                        )
                    })
                    .collect();
                let primary = ranges.len() - 1;
                Selections::new(ranges, primary)
            };
            self.edit(|ed| ed.set_selections(selections));
            self.reveal_primary();
            return true;
        }
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
        self.mouse_up_with(false);
    }

    /// Ends a press. A text drag drops its text where the pointer is: moved, or copied
    /// with `copy` (Ctrl, or Alt on macOS, as in VS Code). A press inside a selection that
    /// did not move places the caret there.
    #[wasm_bindgen(js_name = mouseUpWith)]
    pub fn mouse_up_with(&mut self, copy: bool) {
        self.edit(|ed| ed.check_snippet());
        self.minimap_drag = None;
        self.scrollbar_drag = None;
        self.drag = None;
        let Some((range, drop, x, y)) = self.text_drag.take() else { return };
        let Some(at) = drop else {
            let pos = self.view.hit_test(self.model.borrow().rope(), x as f32, y as f32);
            self.edit(|ed| ed.set_selections(Selections::single(Selection::cursor(pos))));
            return;
        };
        if !copy && at >= range.start && at <= range.end {
            return;
        }
        let text: String = self.model.borrow().rope().slice(range.clone()).to_string();
        let len = range.len();
        let mut edits = vec![rustaco_core::Edit::insert(at, text)];
        let start = if copy {
            at
        } else {
            edits.push(rustaco_core::Edit::delete(range.start, range.end));
            if at > range.end { at - len } else { at }
        };
        self.edit(|ed| {
            ed.apply_edits(edits);
            ed.set_selections(Selections::single(Selection::new(start, start + len)));
        });
        self.reveal_primary();
    }

    /// Whether the point is inside a non-empty selection (a right click there keeps it).
    #[wasm_bindgen(js_name = insideSelection)]
    pub fn inside_selection(&mut self, x: f64, y: f64) -> bool {
        let model = self.model.borrow();
        self.view.sync(&model);
        self.cursors.sync(&model);
        let pos = self.view.hit_test(model.rope(), x as f32, y as f32);
        self.cursors.selections().iter().any(|s| !s.is_empty() && s.start() <= pos && pos <= s.end())
    }

    /// Where dragged text would drop, while a text drag is under way.
    #[wasm_bindgen(js_name = dropPosition)]
    pub fn drop_position(&self) -> Option<u32> {
        self.text_drag.as_ref().and_then(|d| d.1).map(|p| p as u32)
    }

    pub fn render(&mut self, caret_visible: bool) -> Result<(), JsValue> {
        let mut gl = self.gl.take();
        let (cw, lh) = (self.view.metrics.char_width, self.view.metrics.line_height);
        match gl.as_mut() {
            Some(g) => {
                let mut p = GlPainter::new(g, cw, lh, self.dpr);
                self.paint(&mut p, caret_visible);
            }
            None => {
                let (ctx, font) = (self.ctx.clone(), self.font.clone());
                ctx.set_transform(self.dpr, 0.0, 0.0, self.dpr, 0.0, 0.0)?;
                ctx.set_font(&font);
                let mut p = CanvasPainter { ctx: &ctx, font: &font, line_height: lh, dpr: self.dpr };
                self.paint(&mut p, caret_visible);
            }
        }
        self.gl = gl;
        Ok(())
    }
}

impl Editor {
    /// Paints a frame, back to front, through `p`.
    fn paint(&mut self, p: &mut dyn Painter, caret_visible: bool) {
        let bracket_pair = self.current_bracket_pair();
        let model = self.model.borrow();
        self.cursors.sync(&model);
        let selections = self.cursors.selections();
        self.highlights.sync(&model);
        let frame = self.view.layout(&model, selections, &self.highlights);
        let (w, h) = (self.view.width as f64, self.view.height as f64);
        let gutter = frame.gutter_width as f64;
        let cw = self.view.metrics.char_width as f64;
        let lh = self.view.metrics.line_height as f64;
        let fill = |p: &mut dyn Painter, color: &str, r: &Rect| {
            p.rect(color, r.x as f64, r.y as f64, r.width as f64, r.height as f64)
        };

        let chrome = CHROME.with(|c| c.borrow().clone());
        p.rect(&chrome.background, 0.0, 0.0, w, h);
        if let Some(band) = &frame.current_line {
            fill(p, &chrome.current_line, band);
        }
        let types: Vec<DecorationType> = DECORATION_TYPES.with(|t| t.borrow().clone());
        let type_of = |kind: u32| kind.checked_sub(FIRST_DECORATION_TYPE).and_then(|i| types.get(i as usize));
        for (r, kind) in
            self.view.line_bands(&model, |k| type_of(k).is_some_and(|t| t.whole_line && t.background.is_some()))
        {
            if let Some(bg) = type_of(kind).and_then(|t| t.background.as_ref()) {
                fill(p, bg, &r);
            }
        }

        let minimap_width = self.view.minimap_width() as f64;
        p.clip(gutter, 0.0, w - gutter - minimap_width, h);
        // 1px lines at whole device pixels, so they are not blurred across two.
        let dpr = p.dpr();
        let snap = |x: f32| (x as f64 * dpr).floor() / dpr;
        let one_pixel = 1.0 / dpr;
        if self.options.guides {
            let cursor_line = model.rope().char_to_line(selections.primary().head);
            for g in self.view.indent_guides(&model, cursor_line) {
                let color = if g.active { &chrome.indent_guide_active } else { &chrome.indent_guide };
                p.rect(color, snap(g.x), g.top as f64, one_pixel, (g.bottom - g.top) as f64);
            }
        }
        for x in self.view.ruler_positions(&model, &self.options.rulers) {
            p.rect(&chrome.ruler, snap(x), 0.0, one_pixel, h);
        }
        let decorations = self.view.decoration_rects(&model);
        for (r, kind) in &decorations {
            if *kind == FIND_MATCH {
                fill(p, &chrome.find_match, r);
            } else if let Some(bg) = type_of(*kind).filter(|t| !t.whole_line).and_then(|t| t.background.as_ref()) {
                fill(p, bg, r);
            }
        }
        if self.options.occurrences {
            for r in self.occurrence_rects(&model, selections) {
                fill(p, &chrome.word_highlight, &r);
            }
        }
        for r in &frame.selections {
            fill(p, &chrome.selection, r);
        }
        for r in &frame.fold_ellipses {
            let inset = Rect { y: r.y + 2.0, height: r.height - 4.0, ..*r };
            fill(p, &chrome.word_highlight, &inset);
            let (x, y) = ((r.x + r.width / 2.0) as f64, (r.y + r.height / 2.0) as f64);
            p.label("\u{22ef}", x, y, Align::Center, &chrome.line_number);
        }
        p.text_runs(&frame.lines);
        if self.options.render_whitespace {
            for (x, y, _cells, tab) in self.view.whitespace_marks(&model) {
                let mark = if tab { "\u{2192}" } else { "\u{b7}" };
                p.label(mark, x as f64, y as f64 + lh / 2.0, Align::Left, &chrome.whitespace);
            }
        }
        for (r, kind) in &decorations {
            if *kind == COMPOSITION {
                p.underline(r, 1, &chrome.caret);
                continue;
            }
            let Some(t) = type_of(*kind).filter(|t| t.underline != 0 && !t.whole_line) else { continue };
            p.underline(r, t.underline, &t.underline_color);
        }
        if let Some((open, close)) = bracket_pair {
            for at in [open, close] {
                for r in self.view.char_rects(&model, at) {
                    p.outline(&chrome.bracket_match, &r);
                }
            }
        }
        if caret_visible {
            for r in &frame.carets {
                fill(p, &chrome.caret, r);
            }
        }
        p.unclip();
        self.sticky = frame.sticky.iter().map(|l| (l.y, l.height, l.line)).collect();
        self.zone_rects = frame.zones.clone();

        p.rect(&chrome.gutter, 0.0, 0.0, gutter, h);
        for (y, height, kind) in self.view.gutter_marks(&model, |k| type_of(k).is_some_and(|t| t.gutter.is_some())) {
            if let Some(color) = type_of(kind).and_then(|t| t.gutter.as_ref()) {
                p.rect(color, 2.0, y as f64, 3.0, height as f64);
            }
        }
        let rope = model.rope();
        let primary_line = rope.char_to_line(selections.primary().head);
        for line in frame.lines.iter().filter(|l| l.row == 0) {
            let color = if line.line == primary_line { &chrome.line_number_active } else { &chrome.line_number };
            let y = (line.y + line.height / 2.0) as f64;
            p.label(&(line.line + 1).to_string(), gutter - 3.5 * cw, y, Align::Right, color);
        }
        for m in &frame.fold_markers {
            let (cx, cy) = (gutter - 1.5 * cw, (m.y + m.height / 2.0) as f64);
            p.triangle(&chrome.line_number, cx, cy, (cw * 0.35).max(3.0), m.folded);
        }

        if !frame.sticky.is_empty() {
            let text_right = w - minimap_width;
            let bottom = frame.sticky.iter().map(|l| l.y + l.height).fold(0.0f32, f32::max) as f64;
            p.rect(&chrome.background, 0.0, 0.0, text_right, bottom);
            p.clip(gutter, 0.0, text_right - gutter, bottom);
            p.text_runs(&frame.sticky);
            p.unclip();
            for l in &frame.sticky {
                let y = (l.y + l.height / 2.0) as f64;
                p.label(&(l.line + 1).to_string(), gutter - 3.5 * cw, y, Align::Right, &chrome.line_number);
            }
            p.rect(&chrome.indent_guide, 0.0, bottom - 1.0, text_right, 1.0);
        }
        if let Some(m) = &frame.minimap {
            p.rect(&chrome.background, m.x as f64, 0.0, m.width as f64, h);
            STYLES.with(|styles| {
                let styles = styles.borrow();
                let row = (m.row_height - 0.5).max(1.0) as f64;
                let blocks: Vec<(f64, f64, f64, f64, &str)> = m
                    .blocks
                    .iter()
                    .map(|b| {
                        let color = styles.get(b.style as usize).unwrap_or(&styles[0]).color.as_str();
                        (b.x as f64, b.y as f64, b.width as f64, row, color)
                    })
                    .collect();
                p.blocks(&blocks, 0.7);
            });
            fill(p, &chrome.scrollbar, &m.slider);
        }
        // The marks are recomputed when asked (refreshOverview), not on every keystroke: with
        // thousands of search matches that would cost milliseconds per key.
        if self.overview.is_none() {
            let marks = self.view.overview_marks(&model, h as f32, |k| {
                k == FIND_MATCH || type_of(k).is_some_and(|t| t.overview.is_some())
            });
            self.overview = Some(marks);
        }
        for &(y, height, kind) in self.overview.as_deref().unwrap_or_default() {
            let color = if kind == FIND_MATCH {
                Some(&chrome.find_match)
            } else {
                type_of(kind).and_then(|t| t.overview.as_ref())
            };
            if let Some(color) = color {
                p.rect(color, w - 10.0, y as f64, 10.0, height as f64);
            }
        }
        if let Some((top, thumb)) = self.vertical_thumb(rope) {
            p.rect(&chrome.scrollbar, w - 10.0, top, 10.0, thumb);
        }
        if let Some((left, thumb)) = self.horizontal_thumb(rope) {
            p.rect(&chrome.scrollbar, left, h - 10.0, thumb, 10.0);
        }
        if let Some(r) = self.text_drag.as_ref().and_then(|d| d.1).and_then(|p| self.view.position_rect(rope, p)) {
            fill(p, &chrome.caret, &r);
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
    }
}

/// Width of the minimap column in CSS pixels.
const MINIMAP_WIDTH: f32 = 90.0;

impl Editor {
    /// Parses a snippet, resolving VS Code's variables from this editor's state.
    fn parse_snippet(&mut self, source: &str) -> rustaco_core::Snippet {
        let model = self.model.borrow();
        self.cursors.sync(&model);
        let rope = model.rope();
        let sel = self.cursors.selections().primary();
        let line = rope.char_to_line(sel.head);
        let selected: String = rope.slice(sel.start()..sel.end()).to_string();
        let line_text: String = rope.line(line).chars().take(rustaco_core::line_len(rope, line)).collect();
        let (wa, wb) = word_at(rope, sel.head);
        let word: String = rope.slice(wa..wb).to_string();
        let now = js_sys::Date::new_0();
        let two = |n: u32| format!("{n:02}");
        let resolve = |name: &str| -> Option<String> {
            Some(match name {
                "TM_SELECTED_TEXT" => selected.clone(),
                "TM_CURRENT_LINE" => line_text.clone(),
                "TM_CURRENT_WORD" => word.clone(),
                "TM_LINE_INDEX" => line.to_string(),
                "TM_LINE_NUMBER" => (line + 1).to_string(),
                "CURRENT_YEAR" => now.get_full_year().to_string(),
                "CURRENT_YEAR_SHORT" => two(now.get_full_year() % 100),
                "CURRENT_MONTH" => two(now.get_month() + 1),
                "CURRENT_DATE" => two(now.get_date()),
                "CURRENT_HOUR" => two(now.get_hours()),
                "CURRENT_MINUTE" => two(now.get_minutes()),
                "CURRENT_SECOND" => two(now.get_seconds()),
                "RANDOM" => format!("{:06}", (js_sys::Math::random() * 1e6) as u32),
                "RANDOM_HEX" => format!("{:06x}", (js_sys::Math::random() * 16_777_216.0) as u32),
                "UUID" => {
                    let hex = |n: usize| -> String {
                        (0..n).map(|_| format!("{:x}", (js_sys::Math::random() * 16.0) as u32)).collect()
                    };
                    format!(
                        "{}-{}-4{}-{}{}-{}",
                        hex(8),
                        hex(4),
                        hex(3),
                        ["8", "9", "a", "b"][(js_sys::Math::random() * 4.0) as usize % 4],
                        hex(3),
                        hex(12)
                    )
                }
                _ => return None,
            })
        };
        rustaco_core::Snippet::parse(source, &resolve)
    }

    /// Vertical scrollbar thumb: top and height, when the content is taller than the view.
    fn vertical_thumb(&self, rope: &Rope) -> Option<(f64, f64)> {
        let h = self.view.height as f64;
        let content = self.view.content_height(rope);
        (content > h).then(|| {
            let thumb = (h * h / content).max(20.0);
            (self.view.scroll_top() / (content - h) * (h - thumb), thumb)
        })
    }

    /// Horizontal scrollbar thumb: left and width, when lines are wider than the text area.
    fn horizontal_thumb(&self, rope: &Rope) -> Option<(f64, f64)> {
        let gutter = self.view.gutter_width(rope) as f64;
        let area = self.view.width as f64 - gutter - self.view.minimap_width() as f64 - 10.0;
        let content = self.view.content_width(rope) as f64 - gutter;
        (content > area && area > 20.0).then(|| {
            let thumb = (area * area / content).max(20.0);
            (gutter + self.view.scroll_left() as f64 / (content - area) * (area - thumb), thumb)
        })
    }

    /// Starts a scrollbar drag, or pages to a click on the track. Returns whether the press
    /// was on a scrollbar.
    fn scrollbar_down(&mut self, x: f64, y: f64) -> bool {
        let model = self.model.borrow();
        self.view.sync(&model);
        let rope = model.rope();
        let (w, h) = (self.view.width as f64, self.view.height as f64);
        if x >= w - 10.0
            && let Some((top, thumb)) = self.vertical_thumb(rope)
        {
            let content = self.view.content_height(rope);
            let factor = (content - h) / (h - thumb).max(1.0);
            if !(top..top + thumb).contains(&y) {
                // A click on the track centres the thumb there.
                let target = (y - thumb / 2.0) * factor;
                self.view.scroll_to(rope, target, self.view.scroll_left());
            }
            self.scrollbar_drag = Some((true, y, self.view.scroll_top(), factor));
            return true;
        }
        if y >= h - 10.0
            && let Some((left, thumb)) = self.horizontal_thumb(rope)
        {
            let gutter = self.view.gutter_width(rope) as f64;
            let area = w - gutter - self.view.minimap_width() as f64 - 10.0;
            let content = self.view.content_width(rope) as f64 - gutter;
            let factor = (content - area) / (area - thumb).max(1.0);
            if x >= gutter && !(left..left + thumb).contains(&x) {
                let target = (x - gutter - thumb / 2.0) * factor;
                let top = self.view.scroll_top();
                self.view.scroll_to(rope, top, target as f32);
            }
            if x >= gutter {
                self.scrollbar_drag = Some((false, x, self.view.scroll_left() as f64, factor));
                return true;
            }
        }
        false
    }

    /// The bracket pair at the primary cursor: from the tree when the JS side supplied it,
    /// otherwise by scanning the text with the language's (or default) bracket pairs.
    fn current_bracket_pair(&mut self) -> Option<(usize, usize)> {
        if let Some(pair) = self.tree_brackets {
            return pair;
        }
        let head = self.selections_primary().head;
        let model = self.model.borrow();
        let defaults: Vec<(String, String)> =
            [("(", ")"), ("[", "]"), ("{", "}")].iter().map(|&(a, b)| (a.to_owned(), b.to_owned())).collect();
        let pairs = if model.editing.brackets.is_empty() { &defaults } else { &model.editing.brackets };
        rustaco_core::matching_bracket_text(model.rope(), head, pairs)
    }

    fn selections_primary(&mut self) -> Selection {
        self.cursors.sync(&self.model.borrow());
        self.cursors.selections().primary()
    }

    /// Runs editing commands for this editor against the shared model.
    fn edit<R>(&mut self, f: impl FnOnce(&mut Commands) -> R) -> R {
        let mut model = self.model.borrow_mut();
        f(&mut Commands::new(&mut model, &mut self.cursors))
    }

    /// Opens folds that hide a caret, then scrolls the primary caret into view.
    fn reveal_primary(&mut self) {
        let model = self.model.borrow();
        self.cursors.sync(&model);
        let sels = self.cursors.selections();
        self.view.reveal_positions(&model, sels.iter().map(|s| s.head));
        self.view.reveal(model.rope(), sels.primary().head);
    }

    /// Moves carets by shown lines, stepping over folds.
    fn move_vertical(&mut self, lines: isize, extend: bool) {
        let targets: Vec<(usize, Option<usize>)> = {
            let model = self.model.borrow();
            self.cursors.sync(&model);
            self.view.sync(&model);
            let rope = model.rope();
            self.cursors
                .selections()
                .iter()
                .map(|s| self.view.vertical_target(rope, s.head, s.goal_column, lines))
                .collect()
        };
        let mut targets = targets.into_iter();
        self.edit(|ed| ed.move_heads(extend, |_, s| targets.next().unwrap_or((s.head, s.goal_column))));
    }

    /// Runs a folding command; None when `command` is not one.
    fn fold_command(&mut self, command: &str) -> Option<bool> {
        self.overview = None;
        let model = self.model.borrow();
        self.cursors.sync(&model);
        let lines: Vec<usize> = self.cursors.selections().iter().map(|s| model.rope().char_to_line(s.head)).collect();
        let view = &mut self.view;
        let handled = match command {
            "editor.fold" => lines.iter().fold(false, |acc, &l| view.fold(&model, l) | acc),
            "editor.unfold" => lines.iter().fold(false, |acc, &l| view.unfold(&model, l) | acc),
            "editor.toggleFold" => lines.iter().fold(false, |acc, &l| view.toggle_fold(&model, l) | acc),
            "editor.foldAll" => {
                view.fold_all(&model);
                true
            }
            "editor.unfoldAll" => {
                view.unfold_all(&model);
                true
            }
            _ => return None,
        };
        drop(model);
        // A caret inside the region just folded moves to its header line first.
        self.carets_out_of_folds();
        self.reveal_primary();
        Some(handled)
    }

    /// Moves carets that a new fold hid to the end of the fold's header line.
    fn carets_out_of_folds(&mut self) {
        let targets: Vec<usize> = {
            let model = self.model.borrow();
            self.cursors.sync(&model);
            let rope = model.rope();
            self.cursors
                .selections()
                .iter()
                .map(|s| {
                    let line = rope.char_to_line(s.head);
                    if !self.view.is_hidden(line) {
                        return s.head;
                    }
                    rustaco_core::line_end(rope, self.view.shown_line_for(line))
                })
                .collect()
        };
        let mut targets = targets.into_iter();
        self.edit(|ed| ed.move_heads(false, |_, s| (targets.next().unwrap_or(s.head), None)));
    }
}

impl Editor {
    /// Rectangles of the other whole-word occurrences, on the visible lines, of the word at an
    /// empty primary cursor.
    fn occurrence_rects(&self, model: &TextModel, selections: &Selections) -> Vec<Rect> {
        let primary = selections.primary();
        if !primary.is_empty() || selections.len() > 1 {
            return Vec::new();
        }
        let rope = model.rope();
        let (a, b) = word_at(rope, primary.head);
        let word: String = rope.slice(a..b).to_string();
        if word.is_empty() || !word.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Vec::new();
        }
        let visible = self.view.visible_lines(rope);
        let start = rope.line_to_char(visible.start);
        let end = rope.line_to_char(visible.end.min(rope.len_lines()));
        let text: String = rope.slice(start..end).to_string();
        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        let mut rects = Vec::new();
        let mut chars_before = 0;
        let mut last = 0;
        for (i, _) in text.match_indices(word.as_str()) {
            let before_ok = text[..i].chars().next_back().is_none_or(|c| !is_word(c));
            let after_ok = text[i + word.len()..].chars().next().is_none_or(|c| !is_word(c));
            chars_before += text[last..i].chars().count();
            last = i;
            if before_ok && after_ok {
                let at = start + chars_before;
                if at != a {
                    rects.extend(self.view.char_range_rects(model, at..at + (b - a)));
                }
            }
        }
        rects
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

#[cfg(test)]
mod highlight_tests {
    use super::{StyleSpan, shift_spans};

    fn spans(v: &[(usize, usize)]) -> Vec<StyleSpan> {
        v.iter().map(|&(start, end)| StyleSpan { start, end, style: 1 }).collect()
    }

    #[test]
    fn spans_follow_edits_inside_a_line() {
        // Typing inside a word grows it and moves the spans after it.
        let mut s = spans(&[(0, 2), (3, 7), (8, 10)]);
        shift_spans(&mut s, 5, 0, 1);
        assert_eq!(s, spans(&[(0, 2), (3, 8), (9, 11)]));
        // Typing at the end of a word extends it.
        let mut s = spans(&[(3, 7), (8, 10)]);
        shift_spans(&mut s, 7, 0, 1);
        assert_eq!(s, spans(&[(3, 8), (9, 11)]));
        // Typing before a span moves it.
        let mut s = spans(&[(3, 7)]);
        shift_spans(&mut s, 3, 0, 2);
        assert_eq!(s, spans(&[(5, 9)]));
        // Deleting across spans shrinks the one it starts in and drops the ones inside.
        let mut s = spans(&[(0, 4), (5, 6), (7, 10)]);
        shift_spans(&mut s, 2, 6, 0);
        assert_eq!(s, spans(&[(0, 2), (2, 4)]));
    }
}
