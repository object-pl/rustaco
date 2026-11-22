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
    Cursors, Editor as Commands, EditorId, KeyPress, Keymap, LanguageEditing, Platform, Resolved, Rope, SearchQuery,
    Searcher, Selection, Selections, Stickiness, TextModel, default_keymap, jetbrains_keymap, next_match, word_at,
};
use rustaco_syntax::{Colors, Style, Theme, ThemeRule, themes};
use rustaco_view::{FrameLine, INLAY_STYLE, Metrics, Rect, StyleSpan, Styler, View, indent_fold_ranges};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

mod layers;
use layers::{DomLayer, GlLayer, TextLayer};

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

/// Sets a theme from a VS Code colour theme flattened by the JS side.
///
/// `base` (`"dark"`, `"light"` or `"high-contrast"`, from the theme's `type`) supplies the
/// surface colours the theme leaves out. Token rules: one selector per line in `selectors`,
/// a foreground `0xRRGGBBAA` per selector (0 = unset), font flags per selector (bit 0:
/// fontStyle present, bit 1 bold, bit 2 italic, bit 3 underline). Surface colours: one VS Code
/// `colors` key per line in `color_keys`, with its value in `color_values`.
#[wasm_bindgen(js_name = setThemeRules)]
pub fn set_theme_rules(
    base: &str,
    selectors: &str,
    foregrounds: &[u32],
    font_flags: &[u8],
    color_keys: &str,
    color_values: &[u32],
) {
    let rules = selectors
        .split('\n')
        .zip(foregrounds.iter().zip(font_flags))
        .map(|(selector, (&fg, &flags))| {
            let font = |bit: u8| (flags & 1 != 0).then_some(flags & bit != 0);
            ThemeRule {
                selector: selector.to_owned(),
                style: Style {
                    foreground: (fg != 0).then_some(fg),
                    bold: font(2),
                    italic: font(4),
                    underline: font(8),
                    ..Style::default()
                },
            }
        })
        .collect();
    let mut colors = match base {
        "light" => Colors::LIGHT,
        "high-contrast" => Colors::HIGH_CONTRAST,
        _ => Colors::DARK,
    };
    for (key, &value) in color_keys.split('\n').zip(color_values) {
        if value != 0 {
            colors.set(key, value);
        }
    }
    apply_theme(&Theme { rules, colors });
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
}

#[wasm_bindgen]
impl Model {
    #[wasm_bindgen(constructor)]
    pub fn new(text: &str) -> Model {
        Model { inner: Rc::new(RefCell::new(TextModel::new(text))) }
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
        let editing = rustaco_syntax::info(language).map_or_else(LanguageEditing::default, |info| {
            let c = info.config;
            LanguageEditing {
                line_comment: c.line_comment.map(str::to_owned),
                block_comment: c.block_comment.map(|(a, b)| (a.to_owned(), b.to_owned())),
                auto_closing: c.auto_closing.iter().map(|&(a, b)| (a.to_owned(), b.to_owned())).collect(),
                brackets: c.brackets.iter().map(|&(a, b)| (a.to_owned(), b.to_owned())).collect(),
                indent_after: c.indent_after.iter().map(|&s| s.to_owned()).collect(),
            }
        });
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
    /// What draws the text; with a DOM or WebGL layer, `overlay` takes what goes over it.
    text_layer: TextLayer,
    overlay: Option<(HtmlCanvasElement, CanvasRenderingContext2d)>,
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
        let metrics = Metrics { char_width, line_height: (font_size * 1.5).round() as f32 };
        let id = EditorId(NEXT_EDITOR_ID.with(|n| n.replace(n.get() + 1)));
        let cursors = Cursors::new(id, &model.inner.borrow());
        Ok(Editor {
            model: model.inner.clone(),
            highlights: ExternalHighlights::default(),
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
            text_layer: TextLayer::Canvas,
            overlay: None,
        })
    }

    /// Picks the text layer: `"canvas"` (the default), `"dom"` or `"webgl"`. The last two need
    /// an overlay canvas, stacked over the text layer, for carets, gutter and scrollbars.
    #[wasm_bindgen(js_name = setTextLayer)]
    pub fn set_text_layer(&mut self, kind: &str, overlay: Option<HtmlCanvasElement>) -> Result<(), JsValue> {
        self.text_layer = match kind {
            "dom" => TextLayer::Dom(DomLayer::default()),
            "webgl" => TextLayer::Gl(GlLayer::default()),
            _ => TextLayer::Canvas,
        };
        self.overlay = match (&self.text_layer, overlay) {
            (TextLayer::Canvas, _) | (_, None) => None,
            (_, Some(c)) => {
                let ctx: CanvasRenderingContext2d =
                    c.get_context("2d")?.ok_or_else(|| JsValue::from_str("canvas has no 2d context"))?.dyn_into()?;
                c.set_width(self.canvas.width());
                c.set_height(self.canvas.height());
                Some((c, ctx))
            }
        };
        Ok(())
    }

    /// DOM layer changes of the last frame: op, row id, x, y per row (see `layers::DomLayer`).
    #[wasm_bindgen(js_name = domOps)]
    pub fn dom_ops(&self) -> Vec<f64> {
        match &self.text_layer {
            TextLayer::Dom(d) => d.ops.clone(),
            _ => Vec::new(),
        }
    }

    /// HTML of the rows the last frame's op-1 entries set, each ended by a NUL.
    #[wasm_bindgen(js_name = domHtml)]
    pub fn dom_html(&self) -> String {
        match &self.text_layer {
            TextLayer::Dom(d) => d.html.clone(),
            _ => String::new(),
        }
    }

    /// Makes the next frame set every DOM row again.
    #[wasm_bindgen(js_name = domReset)]
    pub fn dom_reset(&mut self) {
        if let TextLayer::Dom(d) = &mut self.text_layer {
            d.reset();
        }
    }

    /// WebGL layer of the last frame: pointer into WASM memory and number of glyph instances
    /// (four 32-bit words each: x, y as f32, slot, RGBA).
    #[wasm_bindgen(js_name = glInstances)]
    pub fn gl_instances(&self) -> Vec<u32> {
        match &self.text_layer {
            TextLayer::Gl(g) => vec![g.instances.as_ptr() as u32, (g.instances.len() / 4) as u32],
            _ => vec![0, 0],
        }
    }

    /// Slot width, height, left padding and slots per row of the atlas, the atlas side, and 1
    /// when the atlas was cleared this frame.
    #[wasm_bindgen(js_name = glGeometry)]
    pub fn gl_geometry(&self) -> Vec<u32> {
        match &self.text_layer {
            TextLayer::Gl(g) => {
                let (w, h, pad, cols) = g.geometry;
                vec![w, h, pad, cols, layers::ATLAS, g.reset as u32]
            }
            _ => Vec::new(),
        }
    }

    /// Glyphs to draw into the atlas: slot, font variant and quarter-pixel offset per glyph.
    #[wasm_bindgen(js_name = glPending)]
    pub fn gl_pending(&self) -> Vec<u32> {
        match &self.text_layer {
            TextLayer::Gl(g) => g.pending.clone(),
            _ => Vec::new(),
        }
    }

    /// The chars of [`Editor::gl_pending`], one per glyph.
    #[wasm_bindgen(js_name = glPendingChars)]
    pub fn gl_pending_chars(&self) -> String {
        match &self.text_layer {
            TextLayer::Gl(g) => g.pending_chars.clone(),
            _ => String::new(),
        }
    }

    pub fn resize(&mut self, width: f64, height: f64, dpr: f64) {
        self.overview = None;
        self.dpr = dpr;
        self.canvas.set_width((width * dpr).round() as u32);
        self.canvas.set_height((height * dpr).round() as u32);
        if let Some((c, _)) = &self.overlay {
            c.set_width(self.canvas.width());
            c.set_height(self.canvas.height());
        }
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
        self.highlights.lines.clear();
        self.add_highlights(first_line, end_line, quads);
    }

    /// Adds highlights for another run of lines, as [`Editor::set_highlights`] takes them.
    #[wasm_bindgen(js_name = addHighlights)]
    pub fn add_highlights(&mut self, first_line: u32, end_line: u32, quads: &[u32]) {
        // The quads describe the text as it is now; semantic tokens move there first.
        self.highlights.sync(&self.model.borrow());
        let lines = &mut self.highlights.lines;
        for line in first_line..end_line {
            lines.insert(line as usize, Vec::new());
        }
        for q in quads.chunks_exact(4) {
            if (first_line..end_line).contains(&q[0]) {
                let spans = lines.entry(q[0] as usize).or_default();
                spans.push(StyleSpan { start: q[1] as usize, end: q[2] as usize, style: q[3] });
            }
        }
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
            Resolved::Command(command) if FRONTEND_COMMANDS.contains(&command.as_str()) => {
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
        self.tree_brackets = Some((pair.len() == 2).then(|| (pair[0] as usize, pair[1] as usize)));
    }

    #[wasm_bindgen(js_name = clearTreeBrackets)]
    pub fn clear_tree_brackets(&mut self) {
        self.tree_brackets = None;
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
        let bracket_pair = self.current_bracket_pair();
        let model = self.model.borrow();
        self.cursors.sync(&model);
        let selections = self.cursors.selections();
        self.highlights.sync(&model);
        let frame = self.view.layout(&model, selections, &self.highlights);
        let ctx = &self.ctx;
        let (w, h) = (self.view.width as f64, self.view.height as f64);
        let gutter = frame.gutter_width as f64;
        let cw = self.view.metrics.char_width as f64;
        let fill = |color: &str, r: &Rect| {
            ctx.set_fill_style_str(color);
            ctx.fill_rect(r.x as f64, r.y as f64, r.width as f64, r.height as f64);
        };

        let chrome = CHROME.with(|c| c.borrow().clone());
        ctx.set_transform(self.dpr, 0.0, 0.0, self.dpr, 0.0, 0.0)?;
        ctx.set_fill_style_str(&chrome.background);
        ctx.fill_rect(0.0, 0.0, w, h);
        if let Some(band) = &frame.current_line {
            fill(&chrome.current_line, band);
        }
        let types: Vec<DecorationType> = DECORATION_TYPES.with(|t| t.borrow().clone());
        let type_of = |kind: u32| kind.checked_sub(FIRST_DECORATION_TYPE).and_then(|i| types.get(i as usize));
        for (r, kind) in
            self.view.line_bands(&model, |k| type_of(k).is_some_and(|t| t.whole_line && t.background.is_some()))
        {
            if let Some(bg) = type_of(kind).and_then(|t| t.background.as_ref()) {
                fill(bg, &r);
            }
        }

        ctx.save();
        ctx.begin_path();
        let minimap_width = self.view.minimap_width() as f64;
        ctx.rect(gutter, 0.0, w - gutter - minimap_width, h);
        ctx.clip();
        // 1px lines at whole device pixels, so they are not blurred across two.
        let dpr = self.dpr;
        let snap = |x: f32| (x as f64 * dpr).floor() / dpr;
        let one_pixel = 1.0 / dpr;
        if self.options.guides {
            let cursor_line = model.rope().char_to_line(selections.primary().head);
            for g in self.view.indent_guides(&model, cursor_line) {
                let color = if g.active { &chrome.indent_guide_active } else { &chrome.indent_guide };
                ctx.set_fill_style_str(color);
                ctx.fill_rect(snap(g.x), g.top as f64, one_pixel, (g.bottom - g.top) as f64);
            }
        }
        ctx.set_fill_style_str(&chrome.ruler);
        for x in self.view.ruler_positions(&model, &self.options.rulers) {
            ctx.fill_rect(snap(x), 0.0, one_pixel, h);
        }
        let decorations = self.view.decoration_rects(&model);
        for (r, kind) in &decorations {
            if *kind == FIND_MATCH {
                fill(&chrome.find_match, r);
            } else if let Some(bg) = type_of(*kind).filter(|t| !t.whole_line).and_then(|t| t.background.as_ref()) {
                fill(bg, r);
            }
        }
        if self.options.occurrences {
            for r in self.occurrence_rects(&model, selections) {
                fill(&chrome.word_highlight, &r);
            }
        }
        for r in &frame.selections {
            fill(&chrome.selection, r);
        }
        for r in &frame.fold_ellipses {
            let inset = Rect { y: r.y + 2.0, height: r.height - 4.0, ..*r };
            fill(&chrome.word_highlight, &inset);
            ctx.set_fill_style_str(&chrome.line_number);
            ctx.set_text_align("center");
            ctx.set_text_baseline("middle");
            ctx.fill_text("\u{22ef}", (r.x + r.width / 2.0) as f64, (r.y + r.height / 2.0) as f64)?;
        }
        let (cw32, lh32) = (self.view.metrics.char_width, self.view.metrics.line_height);
        let ctx = match (&mut self.text_layer, &self.overlay) {
            (TextLayer::Dom(d), Some((_, over))) => {
                d.update(&frame.lines, cw32);
                ctx.restore();
                begin_overlay(over, self.dpr, w, h, gutter, minimap_width)?;
                over
            }
            (TextLayer::Gl(g), Some((_, over))) => {
                g.update(&frame.lines, cw32, lh32, self.dpr);
                ctx.restore();
                begin_overlay(over, self.dpr, w, h, gutter, minimap_width)?;
                over
            }
            _ => {
                ctx.set_text_baseline("middle");
                ctx.set_text_align("left");
                draw_text(ctx, &frame.lines, &self.font, lh32);
                ctx
            }
        };
        let fill = |color: &str, r: &Rect| {
            ctx.set_fill_style_str(color);
            ctx.fill_rect(r.x as f64, r.y as f64, r.width as f64, r.height as f64);
        };
        ctx.set_font(&self.font);
        ctx.set_text_baseline("middle");
        if self.options.render_whitespace {
            ctx.set_fill_style_str(&chrome.whitespace);
            let lh = self.view.metrics.line_height as f64;
            for (x, y, cells, tab) in self.view.whitespace_marks(&model) {
                let mark = if tab { "\u{2192}" } else { "\u{b7}" };
                let _ = cells;
                ctx.fill_text(mark, x as f64, y as f64 + lh / 2.0)?;
            }
        }
        for (r, kind) in &decorations {
            if *kind == COMPOSITION {
                draw_underline(ctx, r, 1, &chrome.caret);
                continue;
            }
            let Some(t) = type_of(*kind).filter(|t| t.underline != 0 && !t.whole_line) else { continue };
            draw_underline(ctx, r, t.underline, &t.underline_color);
        }
        if let Some((open, close)) = bracket_pair {
            ctx.set_stroke_style_str(&chrome.bracket_match);
            ctx.set_line_width(1.0);
            for at in [open, close] {
                for r in self.view.char_rects(&model, at) {
                    ctx.stroke_rect(r.x as f64 + 0.5, r.y as f64 + 0.5, r.width as f64 - 1.0, r.height as f64 - 1.0);
                }
            }
        }
        if caret_visible {
            for r in &frame.carets {
                fill(&chrome.caret, r);
            }
        }
        ctx.restore();
        self.sticky = frame.sticky.iter().map(|l| (l.y, l.height, l.line)).collect();
        self.zone_rects = frame.zones.clone();

        ctx.set_fill_style_str(&chrome.gutter);
        ctx.fill_rect(0.0, 0.0, gutter, h);
        for (y, height, kind) in self.view.gutter_marks(&model, |k| type_of(k).is_some_and(|t| t.gutter.is_some())) {
            if let Some(color) = type_of(kind).and_then(|t| t.gutter.as_ref()) {
                ctx.set_fill_style_str(color);
                ctx.fill_rect(2.0, y as f64, 3.0, height as f64);
            }
        }
        ctx.set_text_align("right");
        let rope = model.rope();
        let primary_line = rope.char_to_line(selections.primary().head);
        for line in frame.lines.iter().filter(|l| l.row == 0) {
            let color = if line.line == primary_line { &chrome.line_number_active } else { &chrome.line_number };
            ctx.set_fill_style_str(color);
            let y = (line.y + line.height / 2.0) as f64;
            ctx.fill_text(&(line.line + 1).to_string(), gutter - 3.5 * cw, y)?;
        }
        ctx.set_fill_style_str(&chrome.line_number);
        for m in &frame.fold_markers {
            // A small triangle: pointing right when folded, down when open.
            let (cx, cy) = (gutter - 1.5 * cw, (m.y + m.height / 2.0) as f64);
            let r = (cw * 0.35).max(3.0);
            ctx.begin_path();
            if m.folded {
                ctx.move_to(cx - r * 0.6, cy - r);
                ctx.line_to(cx + r * 0.6, cy);
                ctx.line_to(cx - r * 0.6, cy + r);
            } else {
                ctx.move_to(cx - r, cy - r * 0.6);
                ctx.line_to(cx + r, cy - r * 0.6);
                ctx.line_to(cx, cy + r * 0.6);
            }
            ctx.close_path();
            ctx.fill();
        }

        if !frame.sticky.is_empty() {
            let text_right = w - self.view.minimap_width() as f64;
            let bottom = frame.sticky.iter().map(|l| l.y + l.height).fold(0.0f32, f32::max) as f64;
            ctx.set_fill_style_str(&chrome.background);
            ctx.fill_rect(0.0, 0.0, text_right, bottom);
            ctx.save();
            ctx.begin_path();
            ctx.rect(gutter, 0.0, text_right - gutter, bottom);
            ctx.clip();
            ctx.set_text_baseline("middle");
            ctx.set_text_align("left");
            draw_text(ctx, &frame.sticky, &self.font, self.view.metrics.line_height);
            ctx.restore();
            ctx.set_font(&self.font);
            ctx.set_text_align("right");
            ctx.set_fill_style_str(&chrome.line_number);
            for l in &frame.sticky {
                ctx.fill_text(&(l.line + 1).to_string(), gutter - 3.5 * cw, (l.y + l.height / 2.0) as f64)?;
            }
            ctx.set_fill_style_str(&chrome.indent_guide);
            ctx.fill_rect(0.0, bottom - 1.0, text_right, 1.0);
        }
        if let Some(m) = &frame.minimap {
            ctx.set_fill_style_str(&chrome.background);
            ctx.fill_rect(m.x as f64, 0.0, m.width as f64, h);
            STYLES.with(|styles| {
                let styles = styles.borrow();
                let mut colors: Vec<&str> = Vec::new();
                let mut ops = Vec::with_capacity(m.blocks.len() * 5);
                let row = (m.row_height - 0.5).max(1.0) as f64;
                for b in &m.blocks {
                    let color = &styles.get(b.style as usize).unwrap_or(&styles[0]).color;
                    let index = colors.iter().position(|c| c == color).unwrap_or_else(|| {
                        colors.push(color);
                        colors.len() - 1
                    });
                    ops.extend([b.x as f64, b.y as f64, b.width as f64, row, index as f64]);
                }
                ctx.set_global_alpha(0.7);
                draw_blocks(ctx, &ops, &colors.join("\n"));
                ctx.set_global_alpha(1.0);
            });
            fill(&chrome.scrollbar, &m.slider);
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
                ctx.set_fill_style_str(color);
                ctx.fill_rect(w - 10.0, y as f64, 10.0, height as f64);
            }
        }
        ctx.set_fill_style_str(&chrome.scrollbar);
        if let Some((top, thumb)) = self.vertical_thumb(rope) {
            ctx.fill_rect(w - 10.0, top, 10.0, thumb);
        }
        if let Some((left, thumb)) = self.horizontal_thumb(rope) {
            ctx.fill_rect(left, h - 10.0, thumb, 10.0);
        }
        if let Some(r) = self.text_drag.as_ref().and_then(|d| d.1).and_then(|p| self.view.position_rect(rope, p)) {
            fill(&chrome.caret, &r);
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

/// Draws the text runs of `lines`. All text goes to JS in one call: one string, one number
/// array, one font change per font. Crossing into JS per run costs more than the drawing.
fn draw_text(ctx: &CanvasRenderingContext2d, lines: &[FrameLine], font: &str, line_height: f32) {
    STYLES.with(|styles| {
        let styles = styles.borrow();
        let inlay = CssStyle {
            color: CHROME.with(|c| c.borrow().line_number.clone()),
            font_prefix: "italic ",
            underline: false,
        };
        let style_of = |id: u32| if id == INLAY_STYLE { &inlay } else { styles.get(id as usize).unwrap_or(&styles[0]) };
        let mut runs: Vec<(&str, u32, f64, f64)> = lines
            .iter()
            .flat_map(|line| {
                let y = (line.y + line.height / 2.0) as f64;
                line.runs.iter().map(move |r| (r.text.as_str(), r.style, r.x as f64, y))
            })
            .collect();
        runs.sort_by_key(|&(_, style, _, _)| (style_of(style).font_prefix, style));

        let mut text = String::new();
        let mut ops: Vec<f64> = Vec::with_capacity(runs.len() * 7);
        let (mut colors, mut fonts): (Vec<&str>, Vec<String>) = (Vec::new(), Vec::new());
        let mut utf16 = 0usize;
        for (run_text, style, x, y) in runs {
            let s = style_of(style);
            let font = format!("{}{}", s.font_prefix, font);
            let font_index = fonts.iter().position(|f| *f == font).unwrap_or_else(|| {
                fonts.push(font);
                fonts.len() - 1
            });
            let color_index = colors.iter().position(|c| *c == s.color).unwrap_or_else(|| {
                colors.push(&s.color);
                colors.len() - 1
            });
            let len: usize = run_text.chars().map(char::len_utf16).sum();
            text.push_str(run_text);
            let underline = if s.underline { (line_height * 0.35) as f64 } else { 0.0 };
            ops.extend([x, y, utf16 as f64, (utf16 + len) as f64, color_index as f64, font_index as f64, underline]);
            utf16 += len;
        }
        draw_runs(ctx, &text, &ops, &colors.join("\n"), &fonts.join("\n"));
    });
}

/// Clears the overlay canvas and clips it to the text area, as the base canvas is at the
/// point the text would be drawn.
fn begin_overlay(
    ctx: &CanvasRenderingContext2d,
    dpr: f64,
    w: f64,
    h: f64,
    gutter: f64,
    minimap: f64,
) -> Result<(), JsValue> {
    ctx.set_transform(dpr, 0.0, 0.0, dpr, 0.0, 0.0)?;
    ctx.clear_rect(0.0, 0.0, w, h);
    ctx.save();
    ctx.begin_path();
    ctx.rect(gutter, 0.0, w - gutter - minimap, h);
    ctx.clip();
    Ok(())
}

/// The DOM layer's CSS, one class per style id (`rx-s<id>`, `rx-si` for inlay hints), for
/// the current theme.
#[wasm_bindgen(js_name = textLayerCss)]
pub fn text_layer_css() -> String {
    layers::dom_css()
}

/// Width of the minimap column in CSS pixels.
const MINIMAP_WIDTH: f32 = 90.0;

/// Draws an underline along the bottom of `r`: 1 solid, 2 wavy, 3 dotted.
fn draw_underline(ctx: &CanvasRenderingContext2d, r: &Rect, style: u8, color: &str) {
    let (x0, x1) = (r.x as f64, (r.x + r.width) as f64);
    let y = (r.y + r.height) as f64 - 2.0;
    ctx.set_stroke_style_str(color);
    ctx.set_fill_style_str(color);
    ctx.set_line_width(1.0);
    match style {
        2 => {
            // A zigzag with a 4 px period, the usual squiggle under diagnostics.
            ctx.begin_path();
            ctx.move_to(x0, y);
            let mut x = x0;
            let mut up = true;
            while x < x1 {
                x = (x + 2.0).min(x1);
                ctx.line_to(x, if up { y - 2.0 } else { y });
                up = !up;
            }
            ctx.stroke();
        }
        3 => {
            let mut x = x0;
            while x < x1 {
                ctx.fill_rect(x, y - 0.5, 1.0, 1.0);
                x += 3.0;
            }
        }
        _ => ctx.fill_rect(x0, y - 0.5, x1 - x0, 1.0),
    }
}

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
