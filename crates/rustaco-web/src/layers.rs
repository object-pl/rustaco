//! The WebGL text layer: the frame's text as quads cut from a glyph atlas. What to draw is
//! computed here and handed to JS as one buffer in WASM memory; JS uploads it and issues a
//! single draw call.
//!
//! With this layer the editor draws on two canvases: everything under the text (current
//! line, selections, guides) on the base canvas, everything over it (carets, underlines of
//! decorations, gutter, minimap, scrollbars, sticky lines) on an overlay canvas, and the
//! WebGL canvas sits between them.

use std::collections::HashMap;

use rustaco_view::{FrameLine, INLAY_STYLE};

use super::{CHROME, CssStyle, STYLES};

/// Atlas side in device pixels.
pub const ATLAS: u32 = 2048;

/// Words per instance: x, y, width, height (f32, device pixels), slot, RGBA.
pub const INSTANCE_WORDS: usize = 6;

/// Slot 0 is filled solid: instances using it are plain rectangles (underlines).
const SOLID: u32 = 0;

/// Glyph quads for WebGL. Each glyph (char, font variant, quarter-pixel offset) is drawn
/// once by JS into a slot of an atlas texture; a frame is a list of instances
/// ([`INSTANCE_WORDS`] 32-bit words each) that JS reads straight from WASM memory.
#[derive(Default)]
pub struct GlLayer {
    slots: HashMap<(char, u8, u8), u32>,
    next_slot: u32,
    /// Slot width and height and left padding in device pixels, and slots per atlas row.
    pub geometry: (u32, u32, u32, u32),
    pub instances: Vec<u32>,
    /// Glyphs to draw into the atlas before the next draw: their chars, and per glyph slot,
    /// font variant (0 normal, 1 italic, 2 bold, 3 bold italic) and quarter-pixel offset.
    /// They pile up over frames until JS takes them, so a frame laid out but never drawn
    /// loses no glyphs.
    pub pending_chars: String,
    pub pending: Vec<u32>,
    /// Set when the atlas was cleared, until JS takes the pending glyphs: JS then clears its
    /// texture and fills the solid slot before drawing them.
    pub reset: bool,
}

fn variant(prefix: &str) -> u8 {
    match prefix {
        "italic " => 1,
        "bold " => 2,
        "italic bold " => 3,
        _ => 0,
    }
}

/// `#rrggbbaa` to the little-endian bytes R, G, B, A of one word.
fn rgba(css: &str) -> u32 {
    u32::from_str_radix(css.trim_start_matches('#'), 16).unwrap_or(0xffffffff).swap_bytes()
}

impl GlLayer {
    /// Hands the pending glyphs and the reset flag to JS and clears them.
    pub fn take_pending(&mut self) -> Vec<u32> {
        self.reset = false;
        self.pending_chars.clear();
        std::mem::take(&mut self.pending)
    }

    /// Forgets the atlas, so the next frame clears it and draws its glyphs again (after the
    /// WebGL context was lost and restored, or the font changed).
    pub fn forget_atlas(&mut self) {
        self.geometry = (0, 0, 0, 0);
    }

    pub fn update(&mut self, lines: &[FrameLine], cw: f32, lh: f32, dpr: f64) {
        let dpr = dpr as f32;
        let pad = (cw * dpr * 0.5).ceil() as u32;
        // Two cells (CJK, emoji) plus room for italic overhang on both sides.
        let slot_w = (2.0 * cw * dpr).ceil() as u32 + 2 * pad;
        let slot_h = (lh * dpr).ceil() as u32;
        let capacity = (ATLAS / slot_w) * (ATLAS / slot_h);
        let geometry = (slot_w, slot_h, pad, ATLAS / slot_w);
        // Start over when the cell size changes or the atlas is three quarters full; the
        // glyphs on screen are drawn again in this frame.
        if geometry != self.geometry || self.next_slot > capacity - capacity / 4 {
            self.geometry = geometry;
            self.slots.clear();
            self.next_slot = SOLID + 1;
            self.reset = true;
            self.pending.clear();
            self.pending_chars.clear();
        }
        self.instances.clear();
        let (sw, sh) = (slot_w as f32, slot_h as f32);
        let underline_h = dpr.round().max(1.0);
        STYLES.with(|styles| {
            let styles = styles.borrow();
            let inlay = CssStyle {
                color: CHROME.with(|c| c.borrow().line_number.clone()),
                font_prefix: "italic ",
                underline: false,
            };
            let style_of =
                |id: u32| if id == INLAY_STYLE { &inlay } else { styles.get(id as usize).unwrap_or(&styles[0]) };
            let mut last_style = u32::MAX;
            let (mut font, mut color, mut underline) = (0u8, 0u32, false);
            for line in lines {
                let y = (line.y * dpr + (line.height * dpr - sh) / 2.0).round();
                for r in &line.runs {
                    if r.style != last_style {
                        last_style = r.style;
                        let s = style_of(r.style);
                        font = variant(s.font_prefix);
                        color = rgba(&s.color);
                        underline = s.underline;
                    }
                    let mut x = r.x;
                    for c in r.text.chars() {
                        if c != ' ' {
                            let dx = x * dpr;
                            let base = dx.floor();
                            let sub = ((dx - base) * 4.0) as u8 & 3;
                            let key = (c, font, sub);
                            let slot = match self.slots.get(&key) {
                                Some(&s) => Some(s),
                                None if self.next_slot < capacity => {
                                    let s = self.next_slot;
                                    self.next_slot += 1;
                                    self.slots.insert(key, s);
                                    self.pending_chars.push(c);
                                    self.pending.extend([s, font as u32, sub as u32]);
                                    Some(s)
                                }
                                None => None,
                            };
                            if let Some(slot) = slot {
                                let x0 = base - pad as f32;
                                self.instances.extend([
                                    x0.to_bits(),
                                    y.to_bits(),
                                    sw.to_bits(),
                                    sh.to_bits(),
                                    slot,
                                    color,
                                ]);
                            }
                        }
                        x += cw;
                    }
                    if underline && !r.text.is_empty() {
                        let x0 = (r.x * dpr).round();
                        let w = ((x * dpr).round() - x0).max(1.0);
                        let uy = (line.y * dpr + line.height * dpr * 0.85).round();
                        self.instances.extend([
                            x0.to_bits(),
                            uy.to_bits(),
                            w.to_bits(),
                            underline_h.to_bits(),
                            SOLID,
                            color,
                        ]);
                    }
                }
            }
        });
    }
}
