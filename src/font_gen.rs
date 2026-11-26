//! Writes a real TrueType font from pixel art.
//!
//! Bevy draws UI text from `Font` assets (TrueType/OpenType). A pixel font is just a font whose
//! outlines are squares, so the game builds its typeface - and its colour emojis - as bitmaps in
//! code (`pixel_font.rs`, `emoji.rs`), and this module turns them into font *bytes* that Bevy loads
//! like any other font. A resource pack can ship a PNG sheet instead (`bitmap_sheet`) and it goes
//! through the same path.
//!
//! One font pixel is [`PIXEL`] units; the em is [`EM_ROWS`] pixels tall (`UNITS_PER_EM`), the
//! baseline sits [`BASELINE_ROW`] pixels below the cell's top. Text is crisp when drawn at a
//! multiple of [`EM_ROWS`] points (`fonts.rs` snaps sizes to that).
//!
//! Colour glyphs use the `COLR` (version 0) + `CPAL` tables: a colour glyph is a stack of
//! single-colour layers, which `cosmic-text` (and so Bevy) renders as a colour image.
//!
//! Everything is pure and tested by parsing the result back.

use std::collections::BTreeMap;

/// Font units per pixel of the pixel art.
pub const PIXEL: i32 = 100;
/// Pixel rows in the em: the cell height of every glyph.
pub const EM_ROWS: i32 = 12;
/// The row boundary the baseline sits on, counted down from the cell's top.
pub const BASELINE_ROW: i32 = 10;
pub const UNITS_PER_EM: i32 = EM_ROWS * PIXEL;

/// One axis-aligned run of ink, in pixel-grid cells: `x0..x1` columns, `y0..y1` rows (row 0 is the
/// top of the cell; rows may go outside `0..EM_ROWS` for a tall glyph).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

/// A solid-colour layer of a glyph; `color` is an index into the font's palette (ignored for a
/// plain monochrome glyph).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layer {
    pub color: u16,
    pub rects: Vec<Rect>,
}

/// One drawable thing mapped to code points.
#[derive(Clone, Debug)]
pub struct GlyphSpec {
    /// The characters that show this glyph (an emoji can have several, e.g. with and without VS16).
    pub chars: Vec<char>,
    /// Advance width in pixels.
    pub advance: i32,
    /// One layer for a mono glyph; several (with palette colours) for a colour glyph.
    pub layers: Vec<Layer>,
    /// Colour glyph: drawn in palette colours (needs `COLR`); mono: one outline in text colour.
    pub colour: bool,
}

pub struct FontSpec {
    pub family: String,
    /// RGBA palette for colour glyphs.
    pub palette: Vec<[u8; 4]>,
    pub glyphs: Vec<GlyphSpec>,
    /// Pixel rows in the em (the cell height) and the row boundary the baseline sits on. The
    /// game's own font uses [`EM_ROWS`] and [`BASELINE_ROW`]; a pack's PNG sheet brings its own.
    pub em_rows: i32,
    pub baseline_row: i32,
    /// Advance of the space and of any code point with no glyph (`.notdef`), in pixels.
    pub space_advance: i32,
}

/// Merges a pixel grid (`rows[y][x]` = ink) into the fewest rectangles: horizontal runs, then
/// identical runs on consecutive rows stacked.
pub fn rects_from_mask(width: usize, rows: &[Vec<bool>], y_offset: i32) -> Vec<Rect> {
    let mut runs: Vec<(i32, i32, i32)> = Vec::new(); // (y, x0, x1)
    for (y, row) in rows.iter().enumerate() {
        let mut x = 0;
        while x < width.min(row.len()) {
            if row[x] {
                let start = x;
                while x < width.min(row.len()) && row[x] {
                    x += 1;
                }
                runs.push((y as i32 + y_offset, start as i32, x as i32));
            } else {
                x += 1;
            }
        }
    }
    let mut out: Vec<Rect> = Vec::new();
    for (y, x0, x1) in runs {
        // Extend a rectangle that ends on the row above with the same columns.
        if let Some(r) = out.iter_mut().find(|r| r.y1 == y && r.x0 == x0 && r.x1 == x1) {
            r.y1 = y + 1;
        } else {
            out.push(Rect { x0, y0: y, x1, y1: y + 1 });
        }
    }
    out
}

// ---------------------------------------------------------------------------
// TrueType writing
// ---------------------------------------------------------------------------

fn be16(v: &mut Vec<u8>, x: i32) {
    v.extend_from_slice(&(x as u16).to_be_bytes());
}
fn be32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_be_bytes());
}

/// A simple glyph: every rectangle a clockwise four-point contour, all on-curve points, 16-bit
/// deltas. Returns the glyph's bytes and its bounding box (xmin, ymin, xmax, ymax) in font units.
fn glyph_bytes(rects: &[Rect], baseline: i32) -> (Vec<u8>, [i32; 4]) {
    if rects.is_empty() {
        return (Vec::new(), [0; 4]);
    }
    let to_x = |x: i32| x * PIXEL;
    let to_y = |y: i32| (baseline - y) * PIXEL;
    let mut pts: Vec<(i32, i32)> = Vec::new();
    let mut ends: Vec<u16> = Vec::new();
    for r in rects {
        // Clockwise in a y-up coordinate system: top-left, top-right, bottom-right, bottom-left.
        pts.push((to_x(r.x0), to_y(r.y0)));
        pts.push((to_x(r.x1), to_y(r.y0)));
        pts.push((to_x(r.x1), to_y(r.y1)));
        pts.push((to_x(r.x0), to_y(r.y1)));
        ends.push((pts.len() - 1) as u16);
    }
    let (xmin, xmax) = (pts.iter().map(|p| p.0).min().unwrap(), pts.iter().map(|p| p.0).max().unwrap());
    let (ymin, ymax) = (pts.iter().map(|p| p.1).min().unwrap(), pts.iter().map(|p| p.1).max().unwrap());
    let mut g = Vec::new();
    be16(&mut g, rects.len() as i32); // numberOfContours
    be16(&mut g, xmin);
    be16(&mut g, ymin);
    be16(&mut g, xmax);
    be16(&mut g, ymax);
    for e in &ends {
        g.extend_from_slice(&e.to_be_bytes());
    }
    be16(&mut g, 0); // instructionLength
    g.extend(std::iter::repeat_n(0x01u8, pts.len())); // on-curve, 16-bit x and y deltas
    let mut last = 0;
    for p in &pts {
        be16(&mut g, p.0 - last);
        last = p.0;
    }
    last = 0;
    for p in &pts {
        be16(&mut g, p.1 - last);
        last = p.1;
    }
    while g.len() % 4 != 0 {
        g.push(0);
    }
    (g, [xmin, ymin, xmax, ymax])
}

fn checksum(data: &[u8]) -> u32 {
    let mut sum = 0u32;
    for chunk in data.chunks(4) {
        let mut b = [0u8; 4];
        b[..chunk.len()].copy_from_slice(chunk);
        sum = sum.wrapping_add(u32::from_be_bytes(b));
    }
    sum
}

/// A name-table string (UTF-16BE, Windows platform).
fn name_table(family: &str) -> Vec<u8> {
    let records: [(u16, String); 6] = [
        (1, family.to_string()),
        (2, "Regular".into()),
        (3, format!("{family} Regular")),
        (4, format!("{family} Regular")),
        (5, "Version 1.0".into()),
        (6, family.replace(' ', "-")),
    ];
    let mut strings = Vec::new();
    let mut recs = Vec::new();
    for (id, text) in &records {
        let enc: Vec<u8> = text.encode_utf16().flat_map(|u| u.to_be_bytes()).collect();
        be16(&mut recs, 3); // platform: Windows
        be16(&mut recs, 1); // encoding: Unicode BMP
        be16(&mut recs, 0x409); // en-US
        be16(&mut recs, *id as i32);
        be16(&mut recs, enc.len() as i32);
        be16(&mut recs, strings.len() as i32);
        strings.extend(enc);
    }
    let mut t = Vec::new();
    be16(&mut t, 0); // format
    be16(&mut t, records.len() as i32);
    be16(&mut t, 6 + 12 * records.len() as i32); // string storage offset
    t.extend(recs);
    t.extend(strings);
    t
}

/// A cmap with a format-4 (BMP) and a format-12 (everything) subtable over `(code point, glyph id)`
/// pairs, merged into runs.
fn cmap_table(map: &BTreeMap<u32, u16>) -> Vec<u8> {
    // Runs of consecutive code points with consecutive glyph ids.
    let mut runs: Vec<(u32, u32, u16)> = Vec::new(); // (start, end, first gid)
    for (&cp, &gid) in map {
        if let Some(last) = runs.last_mut() {
            let len = last.1 - last.0;
            if last.1 + 1 == cp && last.2 as u32 + len + 1 == gid as u32 {
                last.1 = cp;
                continue;
            }
        }
        runs.push((cp, cp, gid));
    }
    // Format 12.
    let mut f12 = Vec::new();
    be16(&mut f12, 12);
    be16(&mut f12, 0);
    be32(&mut f12, 16 + 12 * runs.len() as u32);
    be32(&mut f12, 0);
    be32(&mut f12, runs.len() as u32);
    for (s, e, g) in &runs {
        be32(&mut f12, *s);
        be32(&mut f12, *e);
        be32(&mut f12, *g as u32);
    }
    // Format 4: BMP runs only, plus the mandatory 0xFFFF terminator.
    let bmp: Vec<&(u32, u32, u16)> = runs.iter().filter(|r| r.1 <= 0xFFFF).collect();
    let seg_count = bmp.len() + 1;
    let mut f4 = Vec::new();
    be16(&mut f4, 4);
    let length = 16 + seg_count * 8;
    be16(&mut f4, length as i32);
    be16(&mut f4, 0);
    be16(&mut f4, (seg_count * 2) as i32);
    let mut pow = 1usize;
    let mut log = 0;
    while pow * 2 <= seg_count {
        pow *= 2;
        log += 1;
    }
    be16(&mut f4, (pow * 2) as i32);
    be16(&mut f4, log);
    be16(&mut f4, ((seg_count - pow) * 2) as i32);
    for r in &bmp {
        be16(&mut f4, r.1 as i32);
    }
    be16(&mut f4, 0xFFFF);
    be16(&mut f4, 0); // reservedPad
    for r in &bmp {
        be16(&mut f4, r.0 as i32);
    }
    be16(&mut f4, 0xFFFF);
    for r in &bmp {
        be16(&mut f4, (r.2 as i32 - r.0 as i32) & 0xFFFF);
    }
    be16(&mut f4, 1);
    for _ in 0..seg_count {
        be16(&mut f4, 0); // idRangeOffset
    }
    let mut t = Vec::new();
    be16(&mut t, 0);
    be16(&mut t, 2);
    let off4 = 4 + 2 * 8;
    be16(&mut t, 3);
    be16(&mut t, 1);
    be32(&mut t, off4 as u32);
    be16(&mut t, 3);
    be16(&mut t, 10);
    be32(&mut t, (off4 + f4.len()) as u32);
    t.extend(f4);
    t.extend(f12);
    t
}

/// Builds the font file for `spec`.
pub fn build_ttf(spec: &FontSpec) -> Vec<u8> {
    // Glyph 0 is .notdef (a hollow box), glyph 1 is the space; then, per spec glyph, its layer
    // glyphs and (for colour) a base glyph whose outline is the union silhouette.
    struct Out {
        bytes: Vec<u8>,
        bbox: [i32; 4],
        advance: i32,
    }
    let mut outs: Vec<Out> = Vec::new();
    let push = |rects: &[Rect], advance: i32, outs: &mut Vec<Out>| -> u16 {
        let (bytes, bbox) = glyph_bytes(rects, spec.baseline_row);
        outs.push(Out { bytes, bbox, advance });
        (outs.len() - 1) as u16
    };
    // A hollow box standing on the baseline, five columns wide and as tall as 2/3 of the em.
    let (top, bottom) = (spec.baseline_row - (spec.em_rows * 2 / 3).max(3), spec.baseline_row);
    let notdef = [
        Rect { x0: 1, y0: top, x1: 6, y1: top + 1 },
        Rect { x0: 1, y0: bottom - 1, x1: 6, y1: bottom },
        Rect { x0: 1, y0: top + 1, x1: 2, y1: bottom - 1 },
        Rect { x0: 5, y0: top + 1, x1: 6, y1: bottom - 1 },
    ];
    push(&notdef, 7, &mut outs);
    push(&[], spec.space_advance, &mut outs);
    let mut cmap: BTreeMap<u32, u16> = BTreeMap::new();
    let mut base_records: Vec<(u16, Vec<(u16, u16)>)> = Vec::new();
    for g in &spec.glyphs {
        let gid;
        if g.colour {
            let mut layer_ids = Vec::new();
            let mut all: Vec<Rect> = Vec::new();
            for layer in &g.layers {
                let id = push(&layer.rects, g.advance, &mut outs);
                layer_ids.push((id, layer.color));
                all.extend(layer.rects.iter().copied());
            }
            gid = push(&all, g.advance, &mut outs);
            base_records.push((gid, layer_ids));
        } else {
            let all: Vec<Rect> = g.layers.iter().flat_map(|l| l.rects.iter().copied()).collect();
            gid = push(&all, g.advance, &mut outs);
        }
        for c in &g.chars {
            cmap.insert(*c as u32, gid);
        }
    }
    cmap.insert(' ' as u32, 1);
    base_records.sort_by_key(|r| r.0);
    let num_glyphs = outs.len();

    // glyf + loca (long).
    let mut glyf = Vec::new();
    let mut loca = Vec::new();
    for o in &outs {
        be32(&mut loca, glyf.len() as u32);
        glyf.extend_from_slice(&o.bytes);
    }
    be32(&mut loca, glyf.len() as u32);
    // hmtx.
    let mut hmtx = Vec::new();
    for o in &outs {
        be16(&mut hmtx, o.advance * PIXEL);
        be16(&mut hmtx, o.bbox[0]);
    }
    let units_per_em = spec.em_rows * PIXEL;
    let (mut xmin, mut ymin, mut xmax, mut ymax) = (0, -(spec.em_rows - spec.baseline_row) * PIXEL, 0, units_per_em);
    for o in &outs {
        if !o.bytes.is_empty() {
            xmin = xmin.min(o.bbox[0]);
            ymin = ymin.min(o.bbox[1]);
            xmax = xmax.max(o.bbox[2]);
            ymax = ymax.max(o.bbox[3]);
        }
    }
    let max_advance = outs.iter().map(|o| o.advance * PIXEL).max().unwrap_or(0);

    let mut head = Vec::new();
    be32(&mut head, 0x0001_0000);
    be32(&mut head, 0x0001_0000);
    be32(&mut head, 0); // checkSumAdjustment
    be32(&mut head, 0x5F0F_3CF5);
    be16(&mut head, 0x0003); // flags: baseline at y=0, lsb at x=0
    be16(&mut head, units_per_em);
    head.extend_from_slice(&[0; 16]); // created, modified
    be16(&mut head, xmin);
    be16(&mut head, ymin);
    be16(&mut head, xmax);
    be16(&mut head, ymax);
    be16(&mut head, 0); // macStyle
    be16(&mut head, 8); // lowestRecPPEM
    be16(&mut head, 2); // fontDirectionHint
    be16(&mut head, 1); // indexToLocFormat: long
    be16(&mut head, 0);

    let ascent = spec.baseline_row * PIXEL;
    let descent = -(spec.em_rows - spec.baseline_row) * PIXEL;
    let mut hhea = Vec::new();
    be32(&mut hhea, 0x0001_0000);
    be16(&mut hhea, ascent);
    be16(&mut hhea, descent);
    be16(&mut hhea, 0); // lineGap
    be16(&mut hhea, max_advance);
    be16(&mut hhea, 0);
    be16(&mut hhea, 0);
    be16(&mut hhea, xmax);
    be16(&mut hhea, 1); // caretSlopeRise
    be16(&mut hhea, 0);
    be16(&mut hhea, 0);
    hhea.extend_from_slice(&[0; 8]);
    be16(&mut hhea, 0); // metricDataFormat
    be16(&mut hhea, num_glyphs as i32); // numberOfHMetrics

    let mut maxp = Vec::new();
    be32(&mut maxp, 0x0001_0000);
    be16(&mut maxp, num_glyphs as i32);
    for v in [4096, 64, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0] {
        be16(&mut maxp, v);
    }

    let mut os2 = Vec::new();
    be16(&mut os2, 4); // version
    be16(&mut os2, (max_advance / 2).max(1)); // xAvgCharWidth
    be16(&mut os2, 400);
    be16(&mut os2, 5);
    be16(&mut os2, 0); // fsType
    for v in [600, 600, 0, 300, 600, 600, 0, 300, 50, 300] {
        be16(&mut os2, v); // subscript/superscript/strikeout sizes and positions
    }
    be16(&mut os2, 0); // sFamilyClass
    os2.extend_from_slice(&[0; 10]); // panose
    for _ in 0..4 {
        be32(&mut os2, 0); // ulUnicodeRange
    }
    os2.extend_from_slice(b"CRFT");
    be16(&mut os2, 0x0040); // fsSelection: regular
    let first = cmap.keys().next().copied().unwrap_or(0x20).min(0xFFFF);
    let last = cmap.keys().filter(|c| **c <= 0xFFFF).last().copied().unwrap_or(0x7E);
    be16(&mut os2, first as i32);
    be16(&mut os2, last as i32);
    be16(&mut os2, ascent);
    be16(&mut os2, descent);
    be16(&mut os2, 0);
    be16(&mut os2, ascent);
    be16(&mut os2, -descent);
    be32(&mut os2, 1); // ulCodePageRange1: Latin 1
    be32(&mut os2, 0);
    be16(&mut os2, 5 * PIXEL); // sxHeight
    be16(&mut os2, spec.em_rows * 2 / 3 * PIXEL); // sCapHeight
    be16(&mut os2, 0);
    be16(&mut os2, 32);
    be16(&mut os2, 32);

    let mut post = Vec::new();
    be32(&mut post, 0x0003_0000);
    post.extend_from_slice(&[0; 28]);

    let mut tables: Vec<([u8; 4], Vec<u8>)> = vec![
        (*b"OS/2", os2),
        (*b"cmap", cmap_table(&cmap)),
        (*b"glyf", glyf),
        (*b"head", head),
        (*b"hhea", hhea),
        (*b"hmtx", hmtx),
        (*b"loca", loca),
        (*b"maxp", maxp),
        (*b"name", name_table(&spec.family)),
        (*b"post", post),
    ];
    if !base_records.is_empty() {
        let mut colr = Vec::new();
        let layer_total: usize = base_records.iter().map(|r| r.1.len()).sum();
        be16(&mut colr, 0);
        be16(&mut colr, base_records.len() as i32);
        be32(&mut colr, 14);
        be32(&mut colr, 14 + 6 * base_records.len() as u32);
        be16(&mut colr, layer_total as i32);
        let mut first_layer = 0;
        for (gid, layers) in &base_records {
            be16(&mut colr, *gid as i32);
            be16(&mut colr, first_layer);
            be16(&mut colr, layers.len() as i32);
            first_layer += layers.len() as i32;
        }
        for (_, layers) in &base_records {
            for (lg, color) in layers {
                be16(&mut colr, *lg as i32);
                be16(&mut colr, *color as i32);
            }
        }
        let mut cpal = Vec::new();
        be16(&mut cpal, 0);
        be16(&mut cpal, spec.palette.len() as i32);
        be16(&mut cpal, 1);
        be16(&mut cpal, spec.palette.len() as i32);
        be32(&mut cpal, 14);
        be16(&mut cpal, 0);
        for c in &spec.palette {
            cpal.extend_from_slice(&[c[2], c[1], c[0], c[3]]); // BGRA
        }
        tables.push((*b"COLR", colr));
        tables.push((*b"CPAL", cpal));
    }
    tables.sort_by_key(|t| t.0);

    let n = tables.len();
    let mut out = Vec::new();
    be32(&mut out, 0x0001_0000);
    be16(&mut out, n as i32);
    let mut pow = 1usize;
    let mut log = 0;
    while pow * 2 <= n {
        pow *= 2;
        log += 1;
    }
    be16(&mut out, (pow * 16) as i32);
    be16(&mut out, log);
    be16(&mut out, (n * 16 - pow * 16) as i32);
    let mut offset = 12 + 16 * n;
    let mut head_offset = 0;
    let mut dir = Vec::new();
    let mut body = Vec::new();
    for (tag, data) in &tables {
        if tag == b"head" {
            head_offset = offset;
        }
        dir.extend_from_slice(tag);
        be32(&mut dir, checksum(data));
        be32(&mut dir, offset as u32);
        be32(&mut dir, data.len() as u32);
        body.extend_from_slice(data);
        while body.len() % 4 != 0 {
            body.push(0);
        }
        offset = 12 + 16 * n + body.len();
    }
    out.extend(dir);
    out.extend(body);
    let adjust = 0xB1B0_AFBAu32.wrapping_sub(checksum(&out));
    out[head_offset + 8..head_offset + 12].copy_from_slice(&adjust.to_be_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square_font() -> FontSpec {
        FontSpec {
            family: "Test Pixel".into(),
            palette: vec![[255, 0, 0, 255], [0, 0, 255, 255]],
            space_advance: 4,
            em_rows: EM_ROWS,
            baseline_row: BASELINE_ROW,
            glyphs: vec![
                GlyphSpec { chars: vec!['A'], advance: 5, colour: false, layers: vec![Layer { color: 0, rects: vec![Rect { x0: 0, y0: 2, x1: 3, y1: 10 }] }] },
                GlyphSpec {
                    chars: vec!['\u{1F600}', '\u{263A}'],
                    advance: 13,
                    colour: true,
                    layers: vec![
                        Layer { color: 0, rects: vec![Rect { x0: 0, y0: 0, x1: 12, y1: 12 }] },
                        Layer { color: 1, rects: vec![Rect { x0: 3, y0: 3, x1: 5, y1: 5 }] },
                    ],
                },
            ],
        }
    }

    #[test]
    fn rects_merge_runs_and_stack_rows() {
        let rows: Vec<Vec<bool>> = ["##.#", "##.#", "...#"].iter().map(|r| r.chars().map(|c| c == '#').collect()).collect();
        let rects = rects_from_mask(4, &rows, 0);
        assert_eq!(rects.len(), 2, "{rects:?}");
        assert!(rects.contains(&Rect { x0: 0, y0: 0, x1: 2, y1: 2 }));
        assert!(rects.contains(&Rect { x0: 3, y0: 0, x1: 4, y1: 3 }));
    }

    #[test]
    fn the_font_parses_and_maps_characters_to_outlines() {
        let bytes = build_ttf(&square_font());
        let face = ttf_parser::Face::parse(&bytes, 0).expect("a valid font");
        assert_eq!(face.units_per_em(), UNITS_PER_EM as u16);
        assert_eq!(face.ascender(), (BASELINE_ROW * PIXEL) as i16);
        assert_eq!(face.descender(), -((EM_ROWS - BASELINE_ROW) * PIXEL) as i16);
        let a = face.glyph_index('A').expect("A is mapped");
        assert_eq!(face.glyph_hor_advance(a), Some((5 * PIXEL) as u16));
        let bbox = face.glyph_bounding_box(a).unwrap();
        // Columns 0..3, rows 2..10 -> x 0..300, y from the baseline: row 10 is y 0, row 2 is y 800.
        assert_eq!((bbox.x_min, bbox.y_min, bbox.x_max, bbox.y_max), (0, 0, 300, 800));
        assert!(face.glyph_index(' ').is_some());
        assert_eq!(face.glyph_hor_advance(face.glyph_index(' ').unwrap()), Some(400));
        assert!(face.glyph_index('Z').is_none(), "unmapped characters have no glyph");
        // The family name is what Bevy will see.
        let name = face.names().into_iter().find(|n| n.name_id == 1).and_then(|n| n.to_string());
        assert_eq!(name.as_deref(), Some("Test Pixel"));
    }

    #[test]
    fn an_emoji_is_a_colour_glyph_reachable_from_every_one_of_its_characters() {
        let bytes = build_ttf(&square_font());
        let face = ttf_parser::Face::parse(&bytes, 0).unwrap();
        let g1 = face.glyph_index('\u{1F600}').expect("astral code points work (cmap format 12)");
        let g2 = face.glyph_index('\u{263A}').unwrap();
        assert_eq!(g1, g2);
        assert!(face.is_color_glyph(g1) || face.tables().colr.is_some(), "COLR/CPAL present");
        let colr = face.tables().colr.expect("COLR table");
        assert!(colr.contains(g1));
        assert!(face.raw_face().table(ttf_parser::Tag::from_bytes(b"CPAL")).is_some(), "CPAL table");
    }

    #[test]
    fn a_pixel_is_exactly_a_pixel_unit_wide_so_text_is_crisp_at_multiples_of_the_em() {
        let bytes = build_ttf(&square_font());
        let face = ttf_parser::Face::parse(&bytes, 0).unwrap();
        // At a font size of EM_ROWS points one pixel unit is exactly one pixel.
        let scale = EM_ROWS as f32 / face.units_per_em() as f32;
        assert!((PIXEL as f32 * scale - 1.0).abs() < 1e-6);
    }
}
