//! GM11: one line of horizontal text, rasterised on the CPU — the delivery
//! burn-ins now, the generators part's lettering later (it extends this).
//!
//! The faces are the app's own Plex cuts, embedded ONCE (D16): the app's
//! font stack (`plate::install_fonts`) takes these same statics. Layout is
//! ab_glyph's advances plus pair kerning; coverage lands in a premultiplied
//! [`Image`] like every other render here.

use ab_glyph::{Font, FontRef, GlyphId, PxScale, ScaleFont, point};

use crate::fx::Image;

/// IBM Plex Sans Regular (the app's proportional face).
pub static PLEX_SANS: &[u8] = include_bytes!("../../../app/assets/fonts/IBMPlexSans-Regular.ttf");
/// IBM Plex Mono Medium (the app's monospace face; the burn-ins').
pub static PLEX_MONO: &[u8] = include_bytes!("../../../app/assets/fonts/IBMPlexMono-Medium.ttf");

/// Which embedded face a line is set in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Face {
    Sans,
    #[default]
    Mono,
}

impl Face {
    fn font(self) -> FontRef<'static> {
        let bytes = match self {
            Face::Sans => PLEX_SANS,
            Face::Mono => PLEX_MONO,
        };
        FontRef::try_from_slice(bytes).expect("the embedded Plex face parses")
    }
}

/// One glyph on the line: its id and its pen x from the line's left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub glyph: GlyphId,
    pub x: f32,
}

/// A laid-out line: glyphs left to right, the advance width, and the
/// line's ascent and height (ascent − descent), all in pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub glyphs: Vec<Placed>,
    pub width: f32,
    pub ascent: f32,
    pub height: f32,
}

/// Lay `text` out on one line at `px` pixels (the font's em height).
/// Control characters (a newline included) take no room: one line only.
pub fn layout_line(face: Face, text: &str, px: f32) -> Line {
    let font = face.font();
    let scaled = font.as_scaled(PxScale::from(px.max(1.0)));
    let mut glyphs = Vec::new();
    let mut x = 0.0f32;
    let mut prev: Option<GlyphId> = None;
    for ch in text.chars().filter(|c| !c.is_control()) {
        let id = scaled.glyph_id(ch);
        if let Some(p) = prev {
            x += scaled.kern(p, id);
        }
        glyphs.push(Placed { glyph: id, x });
        x += scaled.h_advance(id);
        prev = Some(id);
    }
    Line {
        glyphs,
        width: x,
        ascent: scaled.ascent(),
        height: scaled.ascent() - scaled.descent(),
    }
}

/// `text` as a premultiplied image just big enough for the line (its
/// advance width × its height, at least 1×1), inked in `color` — a straight
/// RGBA chip whose bytes are the stored values (`c/255`; white and black
/// are the same under either colour law). Transparent where no glyph is.
pub fn raster_line(face: Face, text: &str, px: f32, color: [u8; 4]) -> Image {
    let line = layout_line(face, text, px);
    let (w, h) = (
        line.width.ceil().max(1.0) as u32,
        line.height.ceil().max(1.0) as u32,
    );
    let mut img = Image::new(w, h);
    let font = face.font();
    let scale = PxScale::from(px.max(1.0));
    let c = color.map(|v| v as f32 / 255.0);
    for g in &line.glyphs {
        let glyph = g
            .glyph
            .with_scale_and_position(scale, point(g.x, line.ascent));
        let Some(outline) = font.outline_glyph(glyph) else {
            continue; // a space
        };
        let b = outline.px_bounds();
        outline.draw(|gx, gy, cov| {
            let x = b.min.x as i64 + gx as i64;
            let y = b.min.y as i64 + gy as i64;
            if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
                return;
            }
            let a = cov.clamp(0.0, 1.0) * c[3];
            let i = (y as usize * w as usize + x as usize) * 4;
            // Glyphs of one line may touch (kerned pairs): src-over.
            let keep = 1.0 - a;
            for k in 0..3 {
                img.px[i + k] = c[k] * a + img.px[i + k] * keep;
            }
            img.px[i + 3] = a + img.px[i + 3] * keep;
        });
    }
    img
}
