//! Drawing backends. [`Editor::render`](super::Editor) paints a frame through the
//! [`Painter`] trait; [`CanvasPainter`] draws on a 2D canvas, [`GlPainter`] turns the same
//! calls into one buffer of quads for WebGL.
//!
//! The WebGL buffer holds every rectangle and glyph of the frame in painting order, so JS
//! uploads it and issues one draw call. Glyphs come from an atlas texture: each glyph (char,
//! font variant, light or dark text, quarter-pixel offset) is drawn once by JS into a slot;
//! slot 0 is solid, for rectangles.

use std::collections::HashMap;

use rustaco_view::{FrameLine, INLAY_STYLE, Rect};
use web_sys::CanvasRenderingContext2d;

use super::{CHROME, CssStyle, STYLES, draw_blocks, draw_runs};

/// Horizontal anchor of a label.
#[derive(Clone, Copy, PartialEq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// What [`Editor::render`](super::Editor) draws with. Coordinates are CSS pixels; colours
/// are CSS `#rrggbbaa` strings, as the theme tables hold them.
pub trait Painter {
    fn dpr(&self) -> f64;
    fn rect(&mut self, color: &str, x: f64, y: f64, w: f64, h: f64);
    /// Draws the text runs of `lines`, styled from the theme.
    fn text_runs(&mut self, lines: &[FrameLine]);
    /// A short monospace label (line number, fold ellipsis, whitespace mark) centred
    /// vertically on `y`.
    fn label(&mut self, text: &str, x: f64, y: f64, align: Align, color: &str);
    /// Limits drawing to a rectangle until [`Painter::unclip`]. Not nested.
    fn clip(&mut self, x: f64, y: f64, w: f64, h: f64);
    fn unclip(&mut self);
    /// Minimap blocks: x, y, width, height and colour, drawn at `alpha`.
    fn blocks(&mut self, blocks: &[(f64, f64, f64, f64, &str)], alpha: f64);
    /// A small triangle in the gutter: pointing right when `folded`, down when open.
    fn triangle(&mut self, color: &str, cx: f64, cy: f64, r: f64, folded: bool);

    /// An underline along the bottom of `r`: 1 solid, 2 wavy, 3 dotted.
    fn underline(&mut self, r: &Rect, style: u8, color: &str) {
        let (x0, x1) = (r.x as f64, (r.x + r.width) as f64);
        let y = (r.y + r.height) as f64 - 2.0;
        match style {
            2 => {
                // A zigzag with a 4 px period, the usual squiggle under diagnostics, in
                // one-pixel steps.
                let mut x = x0;
                while x < x1 {
                    let phase = (x - x0) % 4.0;
                    let dy = if phase < 2.0 { phase } else { 4.0 - phase };
                    self.rect(color, x, y - dy - 0.5, 1.0, 1.0);
                    x += 1.0;
                }
            }
            3 => {
                let mut x = x0;
                while x < x1 {
                    self.rect(color, x, y - 0.5, 1.0, 1.0);
                    x += 3.0;
                }
            }
            _ => self.rect(color, x0, y - 0.5, x1 - x0, 1.0),
        }
    }

    /// A one-pixel outline just inside `r`.
    fn outline(&mut self, color: &str, r: &Rect) {
        let one = 1.0 / self.dpr();
        let (x, y, w, h) = (r.x as f64, r.y as f64, r.width as f64, r.height as f64);
        self.rect(color, x, y, w, one);
        self.rect(color, x, y + h - one, w, one);
        self.rect(color, x, y, one, h);
        self.rect(color, x + w - one, y, one, h);
    }
}

fn style_table<R>(f: impl FnOnce(&dyn Fn(u32) -> CssStyle) -> R) -> R {
    STYLES.with(|styles| {
        let styles = styles.borrow();
        let inlay = CssStyle {
            color: CHROME.with(|c| c.borrow().line_number.clone()),
            font_prefix: "italic ",
            underline: false,
        };
        f(&|id: u32| {
            if id == INLAY_STYLE { inlay.clone() } else { styles.get(id as usize).unwrap_or(&styles[0]).clone() }
        })
    })
}

pub struct CanvasPainter<'a> {
    pub ctx: &'a CanvasRenderingContext2d,
    pub font: &'a str,
    pub line_height: f32,
    pub dpr: f64,
}

impl Painter for CanvasPainter<'_> {
    fn dpr(&self) -> f64 {
        self.dpr
    }

    fn rect(&mut self, color: &str, x: f64, y: f64, w: f64, h: f64) {
        self.ctx.set_fill_style_str(color);
        self.ctx.fill_rect(x, y, w, h);
    }

    /// All text goes to JS in one call: one string, one number array, one font change per
    /// font. Crossing into JS per run costs more than the drawing.
    fn text_runs(&mut self, lines: &[FrameLine]) {
        let ctx = self.ctx;
        ctx.set_text_baseline("middle");
        ctx.set_text_align("left");
        style_table(|style_of| {
            let mut runs: Vec<(&str, CssStyle, f64, f64)> = lines
                .iter()
                .flat_map(|line| {
                    let y = (line.y + line.height / 2.0) as f64;
                    line.runs.iter().map(move |r| (r.text.as_str(), style_of(r.style), r.x as f64, y))
                })
                .collect();
            runs.sort_by(|a, b| (a.1.font_prefix, &a.1.color).cmp(&(b.1.font_prefix, &b.1.color)));
            let mut text = String::new();
            let mut ops: Vec<f64> = Vec::with_capacity(runs.len() * 7);
            let (mut colors, mut fonts): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
            let mut utf16 = 0usize;
            for (run_text, s, x, y) in runs {
                let font = format!("{}{}", s.font_prefix, self.font);
                let font_index = fonts.iter().position(|f| *f == font).unwrap_or_else(|| {
                    fonts.push(font);
                    fonts.len() - 1
                });
                let color_index = colors.iter().position(|c| *c == s.color).unwrap_or_else(|| {
                    colors.push(s.color.clone());
                    colors.len() - 1
                });
                let len: usize = run_text.chars().map(char::len_utf16).sum();
                text.push_str(run_text);
                let underline = if s.underline { (self.line_height * 0.35) as f64 } else { 0.0 };
                ops.extend([
                    x,
                    y,
                    utf16 as f64,
                    (utf16 + len) as f64,
                    color_index as f64,
                    font_index as f64,
                    underline,
                ]);
                utf16 += len;
            }
            draw_runs(ctx, &text, &ops, &colors.join("\n"), &fonts.join("\n"));
        });
        ctx.set_font(self.font);
    }

    fn label(&mut self, text: &str, x: f64, y: f64, align: Align, color: &str) {
        let ctx = self.ctx;
        ctx.set_fill_style_str(color);
        ctx.set_text_baseline("middle");
        ctx.set_text_align(match align {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
        });
        let _ = ctx.fill_text(text, x, y);
    }

    fn clip(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.ctx.save();
        self.ctx.begin_path();
        self.ctx.rect(x, y, w, h);
        self.ctx.clip();
    }

    fn unclip(&mut self) {
        self.ctx.restore();
    }

    fn blocks(&mut self, blocks: &[(f64, f64, f64, f64, &str)], alpha: f64) {
        let mut colors: Vec<&str> = Vec::new();
        let mut ops = Vec::with_capacity(blocks.len() * 5);
        for &(x, y, w, h, color) in blocks {
            let index = colors.iter().position(|c| *c == color).unwrap_or_else(|| {
                colors.push(color);
                colors.len() - 1
            });
            ops.extend([x, y, w, h, index as f64]);
        }
        self.ctx.set_global_alpha(alpha);
        draw_blocks(self.ctx, &ops, &colors.join("\n"));
        self.ctx.set_global_alpha(1.0);
    }

    fn triangle(&mut self, color: &str, cx: f64, cy: f64, r: f64, folded: bool) {
        let ctx = self.ctx;
        ctx.set_fill_style_str(color);
        ctx.begin_path();
        if folded {
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

    fn underline(&mut self, r: &Rect, style: u8, color: &str) {
        // Strokes on the canvas: smoother than the one-pixel steps of the default.
        let ctx = self.ctx;
        let (x0, x1) = (r.x as f64, (r.x + r.width) as f64);
        let y = (r.y + r.height) as f64 - 2.0;
        if style != 2 {
            ctx.set_fill_style_str(color);
            if style == 3 {
                let mut x = x0;
                while x < x1 {
                    ctx.fill_rect(x, y - 0.5, 1.0, 1.0);
                    x += 3.0;
                }
            } else {
                ctx.fill_rect(x0, y - 0.5, x1 - x0, 1.0);
            }
            return;
        }
        ctx.set_stroke_style_str(color);
        ctx.set_line_width(1.0);
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
}

/// Atlas side in device pixels.
pub const ATLAS: u32 = 2048;

/// Words per instance: x, y, width, height of the quad, x and y of the glyph slot's origin
/// (all f32, device pixels), slot, RGBA. A quad smaller than the slot is a clipped glyph.
pub const INSTANCE_WORDS: usize = 8;

/// Slot 0 is filled solid: instances using it are plain rectangles.
const SOLID: u32 = 0;

/// The glyph atlas and the frame's quads, kept between frames.
#[derive(Default)]
pub struct GlLayer {
    slots: HashMap<(char, u8, u8), u32>,
    next_slot: u32,
    capacity: u32,
    /// Slot width and height and left padding in device pixels, and slots per atlas row.
    pub geometry: (u32, u32, u32, u32),
    pub instances: Vec<u32>,
    /// Glyphs to draw into the atlas before the next draw: their chars, and per glyph slot,
    /// variant and quarter-pixel offset. The variant is the font (0 normal, 1 italic, 2 bold,
    /// 3 bold italic) plus 4 for dark text, which browsers rasterise thinner than light text.
    /// They pile up over frames until JS takes them, so a frame laid out but never drawn
    /// loses no glyphs.
    pub pending_chars: String,
    pub pending: Vec<u32>,
    /// Set when the atlas was cleared, until JS takes the pending glyphs: JS then clears its
    /// texture and fills the solid slot before drawing them.
    pub reset: bool,
}

impl GlLayer {
    /// Hands the pending glyphs and the reset flag to JS and clears them.
    pub fn take_pending(&mut self) -> Vec<u32> {
        self.reset = false;
        self.pending_chars.clear();
        std::mem::take(&mut self.pending)
    }

    /// Forgets the atlas, so the next frame clears it and draws its glyphs again.
    pub fn forget_atlas(&mut self) {
        self.geometry = (0, 0, 0, 0);
    }

    /// Starts a frame: sizes the slots for the cell size, starting the atlas over when that
    /// changed or it is three quarters full (the glyphs on screen are drawn again).
    fn begin(&mut self, cw: f32, lh: f32, dpr: f32) {
        let pad = (cw * dpr * 0.5).ceil() as u32;
        // Two cells (CJK, emoji) plus room for italic overhang on both sides.
        let slot_w = (2.0 * cw * dpr).ceil() as u32 + 2 * pad;
        let slot_h = (lh * dpr).ceil() as u32;
        let capacity = (ATLAS / slot_w) * (ATLAS / slot_h);
        let geometry = (slot_w, slot_h, pad, ATLAS / slot_w);
        if geometry != self.geometry || self.next_slot > capacity - capacity / 4 {
            self.geometry = geometry;
            self.capacity = capacity;
            self.slots.clear();
            self.next_slot = SOLID + 1;
            self.reset = true;
            self.pending.clear();
            self.pending_chars.clear();
        }
        self.instances.clear();
    }

    fn slot(&mut self, c: char, variant: u8, sub: u8) -> Option<u32> {
        let key = (c, variant, sub);
        if let Some(&s) = self.slots.get(&key) {
            return Some(s);
        }
        if self.next_slot >= self.capacity {
            return None;
        }
        let s = self.next_slot;
        self.next_slot += 1;
        self.slots.insert(key, s);
        self.pending_chars.push(c);
        self.pending.extend([s, variant as u32, sub as u32]);
        Some(s)
    }
}

fn font_variant(prefix: &str) -> u8 {
    match prefix {
        "italic " => 1,
        "bold " => 2,
        "italic bold " => 3,
        _ => 0,
    }
}

/// A CSS colour to the little-endian bytes R, G, B, A of one word. Theme colours are
/// `#rrggbbaa`; decoration types carry what the embedder passed: `#rgb`, `#rgba`, `#rrggbb`,
/// `#rrggbbaa`, `rgb(...)` or `rgba(...)`. Anything else comes out opaque white.
pub fn rgba(css: &str) -> u32 {
    parse_css_color(css.trim())
        .unwrap_or([255, 255, 255, 255])
        .map(|b| b as u32)
        .iter()
        .rev()
        .fold(0, |acc, &b| (acc << 8) | b)
}

fn parse_css_color(css: &str) -> Option<[u8; 4]> {
    if let Some(hex) = css.strip_prefix('#') {
        let digit = |i: usize| u8::from_str_radix(hex.get(i..i + 1)?, 16).ok();
        let pair = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
        return match hex.len() {
            3 | 4 => {
                let a = if hex.len() == 4 { digit(3)? * 17 } else { 255 };
                Some([digit(0)? * 17, digit(1)? * 17, digit(2)? * 17, a])
            }
            6 | 8 => Some([pair(0)?, pair(2)?, pair(4)?, if hex.len() == 8 { pair(6)? } else { 255 }]),
            _ => None,
        };
    }
    let inner = css.strip_prefix("rgba(").or_else(|| css.strip_prefix("rgb("))?.strip_suffix(')')?;
    let parts: Vec<&str> =
        inner.split(|c: char| c == ',' || c == '/' || c.is_whitespace()).filter(|p| !p.is_empty()).collect();
    let channel = |p: &str| -> Option<u8> {
        match p.strip_suffix('%') {
            Some(pct) => Some((pct.parse::<f32>().ok()? * 2.55).round().clamp(0.0, 255.0) as u8),
            None => Some(p.parse::<f32>().ok()?.round().clamp(0.0, 255.0) as u8),
        }
    };
    let alpha = |p: &str| -> Option<u8> {
        match p.strip_suffix('%') {
            Some(pct) => Some((pct.parse::<f32>().ok()? * 2.55).round().clamp(0.0, 255.0) as u8),
            None => Some((p.parse::<f32>().ok()? * 255.0).round().clamp(0.0, 255.0) as u8),
        }
    };
    match parts.as_slice() {
        [r, g, b] => Some([channel(r)?, channel(g)?, channel(b)?, 255]),
        [r, g, b, a] => Some([channel(r)?, channel(g)?, channel(b)?, alpha(a)?]),
        _ => None,
    }
}

/// 4 when the colour is dark enough that its glyphs should be rasterised as dark text.
fn darkness(color: u32) -> u8 {
    let [r, g, b, _] = color.to_le_bytes();
    let luma = 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32;
    if luma < 128.0 { 4 } else { 0 }
}

pub struct GlPainter<'a> {
    gl: &'a mut GlLayer,
    dpr: f32,
    cw: f32,
    clip: Option<[f32; 4]>,
}

impl<'a> GlPainter<'a> {
    pub fn new(gl: &'a mut GlLayer, cw: f32, lh: f32, dpr: f64) -> Self {
        gl.begin(cw, lh, dpr as f32);
        GlPainter { gl, dpr: dpr as f32, cw, clip: None }
    }

    /// Pushes a quad in device pixels, cut to the clip rectangle.
    fn quad(&mut self, [mut x0, mut y0, mut x1, mut y1]: [f32; 4], origin: (f32, f32), slot: u32, color: u32) {
        if let Some([cx0, cy0, cx1, cy1]) = self.clip {
            x0 = x0.max(cx0);
            y0 = y0.max(cy0);
            x1 = x1.min(cx1);
            y1 = y1.min(cy1);
        }
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        self.gl.instances.extend([
            x0.to_bits(),
            y0.to_bits(),
            (x1 - x0).to_bits(),
            (y1 - y0).to_bits(),
            origin.0.to_bits(),
            origin.1.to_bits(),
            slot,
            color,
        ]);
    }

    /// One glyph with its left edge at `x` and centred vertically on `y` (CSS pixels).
    fn glyph(&mut self, c: char, variant: u8, x: f32, y: f32, color: u32) {
        let dx = x * self.dpr;
        let base = dx.floor();
        let sub = ((dx - base) * 4.0) as u8 & 3;
        let Some(slot) = self.gl.slot(c, variant | darkness(color), sub) else { return };
        let (sw, sh, pad, _) = self.gl.geometry;
        let ox = base - pad as f32;
        let oy = (y * self.dpr - sh as f32 / 2.0).round();
        self.quad([ox, oy, ox + sw as f32, oy + sh as f32], (ox, oy), slot, color);
    }
}

impl Painter for GlPainter<'_> {
    fn dpr(&self) -> f64 {
        self.dpr as f64
    }

    fn rect(&mut self, color: &str, x: f64, y: f64, w: f64, h: f64) {
        let d = self.dpr;
        // Edges on whole device pixels, as the canvas fills them.
        let (x0, y0) = ((x as f32 * d).round(), (y as f32 * d).round());
        let (x1, y1) = (((x + w) as f32 * d).round(), ((y + h) as f32 * d).round());
        let (x1, y1) = (x1.max(x0 + 1.0), y1.max(y0 + 1.0));
        self.quad([x0, y0, x1, y1], (x0, y0), SOLID, rgba(color));
    }

    fn text_runs(&mut self, lines: &[FrameLine]) {
        let cw = self.cw;
        style_table(|style_of| {
            let mut last_style = u32::MAX;
            let (mut variant, mut color, mut underline) = (0u8, 0u32, false);
            let mut underline_color = String::new();
            for line in lines {
                let y = line.y + line.height / 2.0;
                for r in &line.runs {
                    if r.style != last_style {
                        last_style = r.style;
                        let s = style_of(r.style);
                        variant = font_variant(s.font_prefix);
                        color = rgba(&s.color);
                        underline = s.underline;
                        underline_color = s.color;
                    }
                    let mut x = r.x;
                    for c in r.text.chars() {
                        if c != ' ' {
                            self.glyph(c, variant, x, y, color);
                        }
                        x += cw;
                    }
                    if underline && !r.text.is_empty() {
                        let uy = (line.y + line.height * 0.85) as f64;
                        let color = underline_color.clone();
                        self.rect(&color, r.x as f64, uy, (x - r.x) as f64, 1.0);
                    }
                }
            }
        });
    }

    fn label(&mut self, text: &str, x: f64, y: f64, align: Align, color: &str) {
        let n = text.chars().count() as f32;
        let x = match align {
            Align::Left => x as f32,
            Align::Center => x as f32 - n * self.cw / 2.0,
            Align::Right => x as f32 - n * self.cw,
        };
        let color = rgba(color);
        for (i, c) in text.chars().enumerate() {
            self.glyph(c, 0, x + i as f32 * self.cw, y as f32, color);
        }
    }

    fn clip(&mut self, x: f64, y: f64, w: f64, h: f64) {
        let d = self.dpr as f64;
        self.clip = Some([
            (x * d).round() as f32,
            (y * d).round() as f32,
            ((x + w) * d).round() as f32,
            ((y + h) * d).round() as f32,
        ]);
    }

    fn unclip(&mut self) {
        self.clip = None;
    }

    fn blocks(&mut self, blocks: &[(f64, f64, f64, f64, &str)], alpha: f64) {
        for &(x, y, w, h, color) in blocks {
            let c = rgba(color);
            let a = ((c >> 24) as f64 * alpha) as u32;
            let c = (c & 0x00ff_ffff) | (a << 24);
            let d = self.dpr;
            let (x0, y0) = ((x as f32 * d).round(), (y as f32 * d).round());
            let (x1, y1) = ((((x + w) as f32) * d).round(), (((y + h) as f32) * d).round());
            self.quad([x0, y0, x1.max(x0 + 1.0), y1.max(y0 + 1.0)], (x0, y0), SOLID, c);
        }
    }

    fn triangle(&mut self, color: &str, cx: f64, cy: f64, r: f64, folded: bool) {
        // Device-pixel rows, as wide as the triangle at each row's middle.
        let d = self.dpr as f64;
        let c = rgba(color);
        let (top, bottom) = if folded { (cy - r, cy + r) } else { (cy - r * 0.6, cy + r * 0.6) };
        let mut y = (top * d).round();
        while y < (bottom * d).round() {
            let mid = (y + 0.5) / d;
            let (x0, x1) = if folded {
                let t = 1.0 - ((mid - cy).abs() / r);
                (cx - r * 0.6, cx - r * 0.6 + t * r * 1.2)
            } else {
                let half = r * (1.0 - (mid - (cy - r * 0.6)) / (r * 1.2));
                (cx - half, cx + half)
            };
            if x1 > x0 {
                let (a, b) = ((x0 * d).round() as f32, (x1 * d).round() as f32);
                self.quad([a, y as f32, b.max(a + 1.0), y as f32 + 1.0], (a, y as f32), SOLID, c);
            }
            y += 1.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::rgba;

    #[test]
    fn css_colours_become_rgba_words() {
        let bytes = |c: &str| rgba(c).to_le_bytes();
        assert_eq!(bytes("#ff000080"), [255, 0, 0, 128]);
        assert_eq!(bytes("#00ff00"), [0, 255, 0, 255]);
        assert_eq!(bytes("#f00"), [255, 0, 0, 255]);
        assert_eq!(bytes("#f008"), [255, 0, 0, 136]);
        assert_eq!(bytes("rgb(1, 2, 3)"), [1, 2, 3, 255]);
        assert_eq!(bytes("rgba(10,20,30,0.5)"), [10, 20, 30, 128]);
        assert_eq!(bytes("rgb(10 20 30 / 50%)"), [10, 20, 30, 128]);
        assert_eq!(bytes("tomato"), [255, 255, 255, 255]);
    }
}
