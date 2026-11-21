//! A system monospace font, rasterized on demand with a glyph cache.

use std::collections::HashMap;

/// A rasterized glyph: coverage per pixel and its placement relative to the pen.
pub struct Glyph {
    pub width: usize,
    pub height: usize,
    /// Pixels from the pen to the bitmap's left edge.
    pub left: i32,
    /// Pixels from the baseline up to the bitmap's top edge.
    pub top: i32,
    pub coverage: Vec<u8>,
}

#[derive(Debug)]
pub enum FontError {
    /// No monospace font among the system fonts.
    NotFound,
    Parse(String),
}

impl std::fmt::Display for FontError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "no monospace font found on this system"),
            Self::Parse(e) => write!(f, "font: {e}"),
        }
    }
}

impl std::error::Error for FontError {}

/// A font at one pixel size, with metrics in physical pixels.
pub struct Font {
    font: fontdue::Font,
    /// Font size in physical pixels.
    pub size: f32,
    /// Advance of one cell.
    pub advance: f32,
    pub ascent: f32,
    pub line_height: f32,
    glyphs: HashMap<char, Glyph>,
}

/// Families tried in order before any monospace font.
const PREFERRED: &[&str] = &[
    "Cascadia Code",
    "Cascadia Mono",
    "Consolas",
    "SF Mono",
    "Menlo",
    "DejaVu Sans Mono",
    "Liberation Mono",
    "Noto Sans Mono",
];

impl Font {
    /// The first preferred family found among the system fonts (or any monospace one) at
    /// `size` physical pixels.
    pub fn system_monospace(size: f32) -> Result<Font, FontError> {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        let mut families: Vec<fontdb::Family> = PREFERRED.iter().map(|n| fontdb::Family::Name(n)).collect();
        families.push(fontdb::Family::Monospace);
        let query = fontdb::Query {
            families: &families,
            weight: fontdb::Weight::NORMAL,
            stretch: fontdb::Stretch::Normal,
            style: fontdb::Style::Normal,
        };
        let id = db.query(&query).ok_or(FontError::NotFound)?;
        let font = db
            .with_face_data(id, |data, index| {
                fontdue::Font::from_bytes(data, fontdue::FontSettings { collection_index: index, ..Default::default() })
            })
            .ok_or(FontError::NotFound)?
            .map_err(|e| FontError::Parse(e.to_string()))?;
        Ok(Self::from_fontdue(font, size))
    }

    /// A font from its file's bytes.
    pub fn from_bytes(bytes: &[u8], size: f32) -> Result<Font, FontError> {
        let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
            .map_err(|e| FontError::Parse(e.to_string()))?;
        Ok(Self::from_fontdue(font, size))
    }

    fn from_fontdue(font: fontdue::Font, size: f32) -> Font {
        let advance = font.metrics('M', size).advance_width;
        let (ascent, line_height) = match font.horizontal_line_metrics(size) {
            Some(m) => (m.ascent.round(), (size * 1.5).round().max((m.ascent - m.descent + m.line_gap).ceil())),
            None => (size.round(), (size * 1.5).round()),
        };
        Font { font, size, advance, ascent, line_height, glyphs: HashMap::new() }
    }

    /// The same face at `factor` times the size (after a DPI change).
    pub fn with_scale(&self, factor: f32) -> Font {
        Self::from_fontdue(self.font.clone(), self.size * factor)
    }

    /// The glyph of `c`, rasterized on first use.
    pub fn glyph(&mut self, c: char) -> &Glyph {
        let (font, size) = (&self.font, self.size);
        self.glyphs.entry(c).or_insert_with(|| {
            let (m, coverage) = font.rasterize(c, size);
            Glyph { width: m.width, height: m.height, left: m.xmin, top: m.ymin + m.height as i32, coverage }
        })
    }
}
