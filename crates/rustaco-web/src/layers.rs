//! Text layers other than the 2D canvas: the frame's text as DOM lines, or as glyph quads
//! for WebGL. Both are computed here and handed to JS as flat arrays; JS only applies them.
//!
//! With either layer, the editor draws on two canvases: everything under the text (current
//! line, selections, guides) on the base canvas, everything over it (carets, underlines,
//! gutter, minimap, scrollbars, sticky lines) on an overlay canvas, and the text layer sits
//! between them.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use rustaco_view::{FrameLine, INLAY_STYLE};
use unicode_width::UnicodeWidthChar;

use super::{CHROME, CssStyle, STYLES};

/// Which text layer an editor draws with.
pub enum TextLayer {
    Canvas,
    Dom(DomLayer),
    Gl(GlLayer),
}

fn with_style<R>(f: impl FnOnce(&dyn Fn(u32) -> CssStyle) -> R) -> R {
    STYLES.with(|styles| {
        let styles = styles.borrow();
        let inlay = CssStyle {
            color: CHROME.with(|c| c.borrow().line_number.clone()),
            font_prefix: "italic ",
            underline: false,
        };
        let style_of = |id: u32| {
            if id == INLAY_STYLE { inlay.clone() } else { styles.get(id as usize).unwrap_or(&styles[0]).clone() }
        };
        f(&style_of)
    })
}

/// CSS for the DOM layer's classes: `.rx-s<id>` per style id, `.rx-si` for inlay hints.
pub fn dom_css() -> String {
    let mut css = String::new();
    let rule = |css: &mut String, class: &str, s: &CssStyle| {
        css.push_str(&format!(".rx-{class}{{color:{}", s.color));
        if s.font_prefix.contains("italic") {
            css.push_str(";font-style:italic");
        }
        if s.font_prefix.contains("bold") {
            css.push_str(";font-weight:bold");
        }
        if s.underline {
            css.push_str(";text-decoration:underline");
        }
        css.push_str("}\n");
    };
    STYLES.with(|styles| {
        for (i, s) in styles.borrow().iter().enumerate() {
            rule(&mut css, &format!("s{i}"), s);
        }
    });
    let inlay =
        CssStyle { color: CHROME.with(|c| c.borrow().line_number.clone()), font_prefix: "italic ", underline: false };
    rule(&mut css, "si", &inlay);
    css
}

#[derive(Clone, Copy)]
struct DomLine {
    hash: u64,
    id: u32,
    x: f32,
    y: f32,
}

/// The text as one absolutely placed `div` per row. Rows are matched to the last frame's by
/// their HTML, so a keystroke rewrites one row and scrolling only moves rows and fills the
/// new ones.
#[derive(Default)]
pub struct DomLayer {
    prev: Vec<DomLine>,
    next_id: u32,
    /// Per row change, four numbers: op (0 move, 1 set HTML and move, 2 remove), row id, x, y.
    pub ops: Vec<f64>,
    /// The HTML of the op-1 rows, in op order, each ended by a NUL.
    pub html: String,
}

fn escape(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\0' => out.push(' '),
            c => out.push(c),
        }
    }
}

fn line_html(line: &FrameLine, cw: f32) -> (String, f32) {
    let mut html = String::new();
    let x0 = line.runs.first().map_or(0.0, |r| r.x);
    let mut x = x0;
    for r in &line.runs {
        let gap = ((r.x - x) / cw).round();
        if gap > 0.0 {
            html.extend(std::iter::repeat_n(' ', gap as usize));
        }
        let mut chars = r.text.chars();
        let single = chars.next().filter(|_| chars.next().is_none());
        let cells = match single {
            Some(c) if !c.is_ascii() => c.width().unwrap_or(1).max(1),
            _ => r.text.chars().count(),
        };
        let class = if r.style == INLAY_STYLE { "si".to_owned() } else { format!("s{}", r.style) };
        match single {
            // A char outside ASCII may come from a fallback font of another width: a box of
            // its own, as wide as its cells, keeps the text after it on the grid.
            Some(c) if !c.is_ascii() => html.push_str(&format!("<span class=\"rx-{class} rx-w{cells}\">")),
            _ => html.push_str(&format!("<span class=\"rx-{class}\">")),
        }
        escape(&mut html, &r.text);
        html.push_str("</span>");
        x = r.x + cells as f32 * cw;
    }
    (html, x0)
}

impl DomLayer {
    /// Computes the changes from the last frame's rows to `lines`.
    pub fn update(&mut self, lines: &[FrameLine], cw: f32) {
        self.ops.clear();
        self.html.clear();
        let mut by_hash: HashMap<u64, Vec<usize>> = HashMap::new();
        for (i, l) in self.prev.iter().enumerate() {
            by_hash.entry(l.hash).or_default().push(i);
        }
        let mut used = vec![false; self.prev.len()];
        let mut next: Vec<DomLine> = Vec::with_capacity(lines.len());
        let mut unmatched: Vec<(usize, String)> = Vec::new();
        for line in lines {
            let (html, x) = line_html(line, cw);
            let mut h = std::collections::hash_map::DefaultHasher::new();
            html.hash(&mut h);
            let hash = h.finish();
            let found = by_hash.get_mut(&hash).and_then(|v| v.pop());
            if let Some(i) = found {
                used[i] = true;
                let old = self.prev[i];
                if old.x != x || old.y != line.y {
                    self.ops.extend([0.0, old.id as f64, x as f64, line.y as f64]);
                }
                next.push(DomLine { hash, id: old.id, x, y: line.y });
            } else {
                unmatched.push((next.len(), html));
                next.push(DomLine { hash, id: u32::MAX, x, y: line.y });
            }
        }
        let mut free: Vec<u32> = self.prev.iter().zip(&used).filter(|(_, u)| !**u).map(|(l, _)| l.id).collect();
        for (i, html) in unmatched {
            let id = free.pop().unwrap_or_else(|| {
                self.next_id += 1;
                self.next_id
            });
            next[i].id = id;
            self.ops.extend([1.0, id as f64, next[i].x as f64, next[i].y as f64]);
            self.html.push_str(&html);
            self.html.push('\0');
        }
        for id in free {
            self.ops.extend([2.0, id as f64, 0.0, 0.0]);
        }
        self.prev = next;
    }

    /// Forgets the rows, so the next frame sets every row again (after a theme change the
    /// HTML stays valid, but after the layer is rebuilt it is gone).
    pub fn reset(&mut self) {
        self.prev.clear();
    }
}

/// Glyph quads for WebGL. Each glyph (char, font variant, quarter-pixel offset) is drawn once
/// into a slot of an atlas texture; a frame is a list of instances, four 32-bit words each:
/// x and y of the slot's top-left in device pixels (f32), the slot index and the colour as
/// RGBA bytes. JS reads the list straight from WASM memory.
#[derive(Default)]
pub struct GlLayer {
    slots: HashMap<(char, u8, u8), u32>,
    next_slot: u32,
    /// Slot width and height and left padding in device pixels, and slots per atlas.
    pub geometry: (u32, u32, u32, u32),
    pub instances: Vec<u32>,
    /// Glyphs to draw into the atlas before this frame: their chars, and per glyph slot,
    /// font variant (0 normal, 1 italic, 2 bold, 3 bold italic) and quarter-pixel offset.
    pub pending_chars: String,
    pub pending: Vec<u32>,
    /// Set when the atlas was cleared: JS clears its texture before drawing the pending glyphs.
    pub reset: bool,
}

/// Atlas side in device pixels.
pub const ATLAS: u32 = 2048;

fn variant(prefix: &str) -> u8 {
    match prefix {
        "italic " => 1,
        "bold " => 2,
        "italic bold " => 3,
        _ => 0,
    }
}

fn rgba(css: &str) -> u32 {
    let v = u32::from_str_radix(css.trim_start_matches('#'), 16).unwrap_or(0xffffffff);
    // 0xRRGGBBAA to little-endian bytes R, G, B, A.
    v.swap_bytes()
}

impl GlLayer {
    pub fn update(&mut self, lines: &[FrameLine], cw: f32, lh: f32, dpr: f64) {
        let dpr = dpr as f32;
        let pad = (cw * dpr * 0.5).ceil() as u32;
        let slot_w = (2.0 * cw * dpr).ceil() as u32 + 2 * pad;
        let slot_h = (lh * dpr).ceil() as u32;
        let capacity = (ATLAS / slot_w) * (ATLAS / slot_h);
        let geometry = (slot_w, slot_h, pad, ATLAS / slot_w);
        self.reset = false;
        self.pending.clear();
        self.pending_chars.clear();
        if geometry != self.geometry || self.next_slot > capacity - capacity / 4 {
            self.geometry = geometry;
            self.slots.clear();
            self.next_slot = 0;
            self.reset = true;
        }
        self.instances.clear();
        with_style(|style_of| {
            let mut last_style = u32::MAX;
            let (mut font, mut color) = (0u8, 0u32);
            for line in lines {
                let y = (line.y * dpr + (line.height * dpr - slot_h as f32) / 2.0).round();
                for r in &line.runs {
                    if r.style != last_style {
                        last_style = r.style;
                        let s = style_of(r.style);
                        font = variant(s.font_prefix);
                        color = rgba(&s.color);
                    }
                    let mut x = r.x;
                    for c in r.text.chars() {
                        if c != ' ' {
                            let dx = x * dpr;
                            let base = dx.floor();
                            let sub = ((dx - base) * 4.0) as u8 & 3;
                            let key = (c, font, sub);
                            let slot = match self.slots.get(&key) {
                                Some(&s) => s,
                                None if self.next_slot < capacity => {
                                    let s = self.next_slot;
                                    self.next_slot += 1;
                                    self.slots.insert(key, s);
                                    self.pending_chars.push(c);
                                    self.pending.extend([s, font as u32, sub as u32]);
                                    s
                                }
                                None => continue,
                            };
                            self.instances.extend([(base - pad as f32).to_bits(), y.to_bits(), slot, color]);
                        }
                        x += cw;
                    }
                }
            }
        });
    }
}
