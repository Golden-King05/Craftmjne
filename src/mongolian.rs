//! Upright Mongolian: words stacked top to bottom along a joined spine, columns running left to right.
//!
//! Mongolian, Todo, Manchu and Sibe are written *downward*, each letter joined to the next along a
//! vertical spine and shaped by where it stands in the word. The game's text engine only lays out
//! horizontal lines, and Unifont (the source of the letter bitmaps, see [`crate::scripts`]) draws only
//! each letter's isolated shape - as upright letters with their spine vertical. This module turns that
//! into proper-looking vertical text:
//!
//! * **Positional forms, derived.** Each letter's spine is found (the column with the most ink; its
//!   longest vertical run). The *initial* form keeps the letter's head (the little hook at the top left)
//!   and drops its tail; the *medial* form drops both; the *final* form drops the head and keeps the
//!   tail; the *isolated* form is the letter as drawn. Joined ends get a spine stub, so neighbouring
//!   letters meet. Which form a letter takes follows from its Unicode joining type and its neighbours
//!   ([`render`]'s state machine). This is an approximation of real calligraphy - every letter has the
//!   same head and tail treatment, where a real font draws each positional form by hand - good enough to
//!   read as joined Mongolian, not a typeface.
//! * **Words** are stacked with all their spines on one line; **columns** wrap at a maximum height and
//!   go left to right, as Mongolian is written. Spaces separate words; a narrow no-break space
//!   (`U+202F`) leaves a small gap inside a word; the Mongolian vowel separator (`U+180E`) breaks the
//!   joining without a gap; free variation selectors are accepted and ignored.
//! * [`VerticalText`] is a UI component that draws a string this way as an image, because a `Text`
//!   node cannot flow downward.
//!
//! Not done: the contextual rules a real shaper applies (some letters take different shapes before
//! certain vowels), the variation selectors' alternate glyphs, rotated digits, and Phags-pa (also
//! written downward, but it does not join).

use std::collections::BTreeMap;
use std::sync::OnceLock;

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::cjk::{Glyph, Join};

/// Rows between two words in a column, and columns between two columns.
const WORD_GAP: usize = 8;
const COLUMN_GAP: usize = 6;
/// Rows of a narrow no-break space, and between two letters that do not join.
const NNBSP_GAP: usize = 4;
const BREAK_GAP: usize = 2;
/// Length of the spine stroke of the Mongolian Nirugu (`U+180A`).
const NIRUGU_LEN: usize = 5;

/// Whether `c` is in the Mongolian block (Mongolian, Todo, Manchu, Sibe) or its supplement.
pub fn is_mongolian_script(c: char) -> bool {
    matches!(c as u32, 0x1800..=0x18AF | 0x11660..=0x1167F)
}

/// A bitmap of ink: `true` is a lit pixel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bitmap {
    pub width: usize,
    pub height: usize,
    px: Vec<bool>,
}

impl Bitmap {
    fn new(width: usize, height: usize) -> Self {
        Self { width, height, px: vec![false; width * height] }
    }

    pub fn get(&self, x: usize, y: usize) -> bool {
        x < self.width && y < self.height && self.px[y * self.width + x]
    }

    fn set(&mut self, x: usize, y: usize) {
        if x < self.width && y < self.height {
            self.px[y * self.width + x] = true;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }
}

fn table() -> &'static BTreeMap<u32, Glyph> {
    static T: OnceLock<BTreeMap<u32, Glyph>> = OnceLock::new();
    T.get_or_init(crate::scripts::glyphs)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    Isolated,
    Initial,
    Medial,
    Final,
}

/// A letter in one form, cropped to its rows, with its spine column.
#[derive(Clone, Debug)]
pub(crate) struct Piece {
    /// `rows[y][x]`, all the glyph's width.
    pub(crate) rows: Vec<Vec<bool>>,
    pub(crate) spine: usize,
}

/// The letter `g` in `form`: see the module docs for the rules.
pub(crate) fn piece(g: &Glyph, form: Form) -> Piece {
    let w = g.width;
    // Two spare rows each side for the join stubs.
    const PAD: usize = 2;
    let mut ink = vec![vec![false; w]; 16 + 2 * PAD];
    for y in 0..16 {
        for x in 0..w {
            ink[y + PAD][x] = g.ink(x, y);
        }
    }
    let count = |ink: &Vec<Vec<bool>>, x: usize| (0..ink.len()).filter(|&y| ink[y][x]).count();
    let spine = (0..w).max_by_key(|&x| (count(&ink, x), std::cmp::Reverse(x))).unwrap_or(0);
    // The longest unbroken vertical run in the spine column.
    let (mut best, mut start) = ((0usize, 0usize), None::<usize>);
    for y in 0..=ink.len() {
        let on = y < ink.len() && ink[y][spine];
        match (on, start) {
            (true, None) => start = Some(y),
            (false, Some(s)) => {
                if y - s > best.1 - best.0 || best == (0, 0) {
                    best = (s, y);
                }
                start = None;
            }
            _ => {}
        }
    }
    let (top, bottom) = (best.0, best.1.saturating_sub(1)); // inclusive
    if best == (0, 0) {
        return Piece { rows: ink[PAD..PAD + 16].to_vec(), spine };
    }
    let strip_head = matches!(form, Form::Medial | Form::Final);
    let strip_tail = matches!(form, Form::Initial | Form::Medial);
    if strip_head {
        for y in 0..ink.len() {
            for x in 0..w {
                // Everything above the spine, and the hook beside its top two rows.
                if y < top || (x < spine && y < top + 2) {
                    ink[y][x] = false;
                }
            }
        }
    }
    if strip_tail {
        for row in ink.iter_mut().skip(bottom + 1) {
            row.iter_mut().for_each(|p| *p = false);
        }
    }
    // The joined ends: one spine pixel beyond the spine.
    if strip_head || form == Form::Medial {
        ink[top - 1][spine] = true;
    }
    if strip_tail {
        ink[bottom + 1][spine] = true;
    }
    let first = (0..ink.len()).find(|&y| ink[y].iter().any(|&p| p));
    let last = (0..ink.len()).rev().find(|&y| ink[y].iter().any(|&p| p));
    match (first, last) {
        (Some(a), Some(b)) => Piece { rows: ink[a..=b].to_vec(), spine },
        _ => Piece { rows: Vec::new(), spine },
    }
}

/// What a character is, for laying out a word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Item {
    /// A letter that joins its neighbours.
    Letter(u32),
    /// A character that stands alone (digits, punctuation, signs).
    Standalone(u32),
    /// The Nirugu: a stroke along the spine that joins the letters on both sides.
    Connector,
    /// A narrow gap inside a word.
    Gap,
    /// The vowel separator: breaks the joining, no gap.
    Break,
}

/// A word as a bitmap with its spine column, and the spine's distance from its left and right edge.
struct Word {
    bmp: Bitmap,
    spine: usize,
}

fn classify(c: char) -> Option<Item> {
    let cp = c as u32;
    match cp {
        0x180B..=0x180D | 0x180F | 0x200D | 0x200C => return None, // variation selectors, joiners: ignored
        0x180E => return Some(Item::Break),
        0x202F => return Some(Item::Gap),
        0x180A => return Some(Item::Connector),
        _ => {}
    }
    let g = table().get(&cp)?;
    if g.combining {
        return None;
    }
    Some(if g.join == Join::Dual { Item::Letter(cp) } else { Item::Standalone(cp) })
}

/// Lays one word out: the pieces stacked along a common spine.
fn word(items: &[Item]) -> Option<Word> {
    // Which neighbours each item joins.
    let joins = |it: &Item| matches!(it, Item::Letter(_) | Item::Connector);
    let mut pieces: Vec<(Piece, usize)> = Vec::new(); // (piece, gap rows above it)
    let mut pending_gap = 0;
    for (i, it) in items.iter().enumerate() {
        let prev_joins = i > 0 && joins(&items[i - 1]) && joins(it);
        let next_joins = i + 1 < items.len() && joins(&items[i + 1]) && joins(it);
        match *it {
            Item::Gap => pending_gap = pending_gap.max(NNBSP_GAP),
            Item::Break => pending_gap = pending_gap.max(0),
            Item::Letter(cp) => {
                let form = match (prev_joins, next_joins) {
                    (false, false) => Form::Isolated,
                    (false, true) => Form::Initial,
                    (true, true) => Form::Medial,
                    (true, false) => Form::Final,
                };
                let p = piece(&table()[&cp], form);
                if p.rows.is_empty() {
                    continue;
                }
                let gap = if prev_joins { 0 } else if pieces.is_empty() { 0 } else { pending_gap.max(BREAK_GAP) };
                pieces.push((p, gap));
                pending_gap = 0;
            }
            Item::Standalone(cp) => {
                let p = piece(&table()[&cp], Form::Isolated);
                if p.rows.is_empty() {
                    continue;
                }
                let gap = if pieces.is_empty() { 0 } else { pending_gap.max(BREAK_GAP) };
                pieces.push((p, gap));
                pending_gap = 0;
            }
            Item::Connector => {
                let spine = 3;
                let rows = vec![{
                    let mut r = vec![false; 8];
                    r[spine] = true;
                    r
                }; NIRUGU_LEN];
                let gap = if prev_joins { 0 } else if pieces.is_empty() { 0 } else { pending_gap.max(BREAK_GAP) };
                pieces.push((Piece { rows, spine }, gap));
                pending_gap = 0;
            }
        }
    }
    if pieces.is_empty() {
        return None;
    }
    let left = pieces.iter().map(|(p, _)| p.spine).max().unwrap();
    let right = pieces.iter().map(|(p, _)| p.rows[0].len() - p.spine).max().unwrap();
    let height: usize = pieces.iter().map(|(p, gap)| p.rows.len() + gap).sum();
    let mut bmp = Bitmap::new(left + right, height);
    let mut y = 0;
    for (p, gap) in &pieces {
        y += gap;
        for row in &p.rows {
            for (x, on) in row.iter().enumerate() {
                if *on {
                    bmp.set(x + left - p.spine, y);
                }
            }
            y += 1;
        }
    }
    Some(Word { bmp, spine: left })
}

/// Splits `text` into words of items, noting where a line break was asked for.
fn words(text: &str) -> Vec<Option<Vec<Item>>> {
    // `None` is a forced new column (a newline).
    let mut out: Vec<Option<Vec<Item>>> = Vec::new();
    let mut cur: Vec<Item> = Vec::new();
    for c in text.chars() {
        if c == '\n' || c.is_whitespace() && c != '\u{202F}' && c != '\u{180E}' {
            if !cur.is_empty() {
                out.push(Some(std::mem::take(&mut cur)));
            }
            if c == '\n' {
                out.push(None);
            }
            continue;
        }
        if let Some(it) = classify(c) {
            cur.push(it);
        }
    }
    if !cur.is_empty() {
        out.push(Some(cur));
    }
    out
}

/// Draws `text` as upright Mongolian: words stacked downward, wrapped into columns of at most
/// `max_height` rows (`0` for no limit) that run left to right.
pub fn render(text: &str, max_height: usize) -> Bitmap {
    // Group the words into columns.
    let mut columns: Vec<Vec<Word>> = vec![Vec::new()];
    let mut height = 0usize;
    for item in words(text) {
        let Some(w) = item.and_then(|items| word(&items)) else {
            if !columns.last().unwrap().is_empty() {
                columns.push(Vec::new());
                height = 0;
            }
            continue;
        };
        let col = columns.last_mut().unwrap();
        let need = if col.is_empty() { w.bmp.height } else { height + WORD_GAP + w.bmp.height };
        if !col.is_empty() && max_height > 0 && need > max_height {
            columns.push(vec![w]);
            height = columns.last().unwrap()[0].bmp.height;
        } else {
            height = need;
            col.push(w);
        }
    }
    columns.retain(|c| !c.is_empty());
    if columns.is_empty() {
        return Bitmap::new(0, 0);
    }
    // Each column shares one spine line.
    struct Col {
        left: usize,
        right: usize,
        height: usize,
    }
    let dims: Vec<Col> = columns
        .iter()
        .map(|c| Col {
            left: c.iter().map(|w| w.spine).max().unwrap(),
            right: c.iter().map(|w| w.bmp.width - w.spine).max().unwrap(),
            height: c.iter().map(|w| w.bmp.height).sum::<usize>() + WORD_GAP * (c.len() - 1),
        })
        .collect();
    let total_w: usize = dims.iter().map(|d| d.left + d.right).sum::<usize>() + COLUMN_GAP * (dims.len() - 1);
    let total_h = dims.iter().map(|d| d.height).max().unwrap();
    let mut out = Bitmap::new(total_w, total_h);
    let mut x0 = 0;
    for (col, d) in columns.iter().zip(&dims) {
        let mut y = 0;
        for w in col {
            for yy in 0..w.bmp.height {
                for xx in 0..w.bmp.width {
                    if w.bmp.get(xx, yy) {
                        out.set(x0 + d.left - w.spine + xx, y + yy);
                    }
                }
            }
            y += w.bmp.height + WORD_GAP;
        }
        x0 += d.left + d.right + COLUMN_GAP;
    }
    out
}

/// Draws a string as upright Mongolian, as an image. Give the entity a `Node` position or let the layout
/// place it; its size is set from the text. Tint it with [`ImageNode::color`].
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct VerticalText {
    pub text: String,
    /// Tallest a column may be, in screen pixels (`0` for no limit); words wrap into the next column.
    pub max_height: u32,
    /// Screen pixels per letter pixel (at least one); 2 sits well beside 24-pixel Latin text.
    pub scale: u32,
}

impl VerticalText {
    pub fn new(text: impl Into<String>, max_height: u32, scale: u32) -> Self {
        Self { text: text.into(), max_height, scale: scale.max(1) }
    }
}

/// Renders every changed [`VerticalText`] into an image and sizes its node to match.
pub fn build_vertical_text(
    mut commands: Commands,
    images: Option<ResMut<Assets<Image>>>,
    mut changed: Query<(Entity, &VerticalText, Option<&mut ImageNode>, Option<&mut Node>), Changed<VerticalText>>,
) {
    let Some(mut images) = images else { return };
    for (entity, vt, node_image, node) in &mut changed {
        let scale = vt.scale.max(1) as usize;
        let max = if vt.max_height == 0 { 0 } else { (vt.max_height as usize / scale).max(1) };
        let bmp = render(&vt.text, max);
        let (w, h) = (bmp.width.max(1) * scale, bmp.height.max(1) * scale);
        let mut data = vec![0u8; w * h * 4];
        for y in 0..bmp.height * scale {
            for x in 0..bmp.width * scale {
                if bmp.get(x / scale, y / scale) {
                    let o = (y * w + x) * 4;
                    data[o..o + 4].copy_from_slice(&[255, 255, 255, 255]);
                }
            }
        }
        let mut image = Image::new(Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default());
        image.sampler = ImageSampler::nearest();
        let handle = images.add(image);
        match node_image {
            Some(mut n) => n.image = handle,
            None => {
                commands.entity(entity).insert(ImageNode::new(handle));
            }
        }
        match node {
            Some(mut n) => {
                n.width = Val::Px(w as f32);
                n.height = Val::Px(h as f32);
            }
            None => {
                commands.entity(entity).insert(Node { width: Val::Px(w as f32), height: Val::Px(h as f32), ..default() });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// MONGOLIAN LETTER MA, O, NGA (three dual-joining letters).
    const MA: char = '\u{182E}';
    const O: char = '\u{1823}';
    const NGA: char = '\u{1829}';
    const NA: char = '\u{1828}';

    fn spine_column(b: &Bitmap) -> usize {
        (0..b.width).max_by_key(|&x| ((0..b.height).filter(|&y| b.get(x, y)).count(), std::cmp::Reverse(x))).unwrap()
    }

    /// Rows between the first and last row that has ink on the spine where it has none: a break in the spine.
    fn spine_breaks(b: &Bitmap) -> Vec<usize> {
        let s = spine_column(b);
        let rows: Vec<usize> = (0..b.height).filter(|&y| b.get(s, y)).collect();
        match (rows.first(), rows.last()) {
            (Some(&a), Some(&z)) => (a..=z).filter(|&y| !b.get(s, y)).collect(),
            _ => vec![0],
        }
    }

    #[test]
    fn a_word_is_one_unbroken_spine_from_top_to_bottom() {
        let word = render(&format!("{MA}{O}{NA}"), 0);
        assert!(!word.is_empty());
        assert!(word.height > 20, "three letters are a tall word, not {} rows", word.height);
        // Where one letter hands over to the next, the spine is inked on both sides of the seam: the
        // letters are joined, not stacked with a gap. (A letter's own detached tail - Unifont's NA has one
        // - may leave a gap *inside* a piece, which is its shape, not a seam.)
        let g = |c: char| table()[&(c as u32)];
        let pieces = [piece(&g(MA), Form::Initial), piece(&g(O), Form::Medial), piece(&g(NA), Form::Final)];
        let left = pieces.iter().map(|p| p.spine).max().unwrap();
        let mut seam = 0;
        for p in &pieces[..2] {
            seam += p.rows.len();
            assert!(word.get(left, seam - 1) && word.get(left, seam), "the spine is broken at the seam below row {seam}");
        }
    }

    #[test]
    fn unjoined_letters_do_not_touch_but_joined_ones_do() {
        let joined = render(&format!("{MA}{O}"), 0);
        let apart = render(&format!("{MA} {O}"), 0);
        assert!(spine_breaks(&joined).is_empty());
        // Two words leave at least the word gap of empty rows between them.
        let breaks = spine_breaks(&apart);
        assert!(breaks.len() >= WORD_GAP - 1, "only {} empty rows between words", breaks.len());
    }

    #[test]
    fn a_letter_takes_a_different_shape_by_position() {
        let g = table()[&(NA as u32)];
        let forms: Vec<Vec<Vec<bool>>> = [Form::Isolated, Form::Initial, Form::Medial, Form::Final].iter().map(|f| piece(&g, *f).rows).collect();
        for i in 0..4 {
            for j in i + 1..4 {
                assert_ne!(forms[i], forms[j], "forms {i} and {j} of NA are identical");
            }
        }
        // The initial form keeps the head and is shorter than the isolated one; the medial has neither.
        assert!(forms[2].len() < forms[0].len());
    }

    #[test]
    fn the_joining_forms_follow_the_neighbours() {
        // First letter initial, last final, middle medial - checked by comparing against the pieces.
        let g = |c: char| table()[&(c as u32)];
        let three = render(&format!("{MA}{O}{NGA}"), 0);
        let expect_height = piece(&g(MA), Form::Initial).rows.len() + piece(&g(O), Form::Medial).rows.len() + piece(&g(NGA), Form::Final).rows.len();
        assert_eq!(three.height, expect_height);
        // A single letter is the isolated form.
        assert_eq!(render(&MA.to_string(), 0).height, piece(&g(MA), Form::Isolated).rows.len());
    }

    #[test]
    fn long_text_wraps_into_columns_that_run_left_to_right() {
        let text = format!("{MA}{O}{NGA} {MA}{O}{NGA} {MA}{O}{NGA} {MA}{O}{NGA}");
        let tall = render(&text, 0);
        let wrapped = render(&text, tall.height / 2);
        assert!(wrapped.width > tall.width, "wrapping adds columns");
        assert!(wrapped.height < tall.height);
        // The first word is in the leftmost column: the left edge holds its ink at the top.
        let first_col_top = (0..wrapped.width / 2).any(|x| wrapped.get(x, 0));
        assert!(first_col_top);
        // A newline starts a new column.
        let broken = render(&format!("{MA}{O}\n{MA}{O}"), 0);
        assert!(broken.width > render(&format!("{MA}{O}"), 0).width);
    }

    #[test]
    fn variation_selectors_are_ignored_the_vowel_separator_breaks_joining_and_nirugu_joins() {
        let plain = render(&format!("{MA}{O}"), 0);
        let fvs = render(&format!("{MA}\u{180B}{O}"), 0);
        assert_eq!(plain, fvs, "a free variation selector changes nothing here");
        // MVS between two letters: both are isolated forms, not initial + final.
        let split = render(&format!("{MA}\u{180E}{O}"), 0);
        let g = |c: char| table()[&(c as u32)];
        let isolated = piece(&g(MA), Form::Isolated).rows.len() + piece(&g(O), Form::Isolated).rows.len();
        assert!(split.height >= isolated, "{} vs {isolated}", split.height);
        // Nirugu is a stroke along the spine joining the letters on both sides.
        let with = render(&format!("{MA}\u{180A}{O}"), 0);
        assert!(spine_breaks(&with).is_empty(), "the spine breaks through the Nirugu: {:?}", spine_breaks(&with));
        let without = render(&format!("{MA}{O}"), 0);
        assert!(with.height > without.height, "the Nirugu adds a stroke");
    }

    #[test]
    fn empty_and_non_mongolian_text_render_nothing_and_do_not_panic() {
        assert!(render("", 0).is_empty());
        assert!(render("hello", 0).is_empty());
        assert!(render("  \n ", 0).is_empty());
    }

    #[test]
    fn manchu_and_todo_letters_shape_too() {
        // MANCHU I, TODO LONG VOWEL SIGN, SIBE UE: all in the block, all dual-joining.
        for text in ["\u{1873}\u{1873}\u{1873}", "\u{1843}\u{1820}\u{1843}", "\u{1860}\u{1860}"] {
            let b = render(text, 0);
            assert!(!b.is_empty(), "{text:?}");
        }
    }

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()));
        app.init_asset::<Image>();
        app.add_systems(Update, build_vertical_text);
        app
    }

    #[test]
    fn the_component_becomes_an_image_sized_to_the_text_and_scale() {
        let mut app = app();
        let e = app.world_mut().spawn(VerticalText::new(format!("{MA}{O}{NGA}"), 0, 3)).id();
        app.update();
        let bmp = render(&format!("{MA}{O}{NGA}"), 0);
        let world = app.world();
        let node = world.get::<Node>(e).expect("the node is sized");
        assert_eq!(node.width, Val::Px((bmp.width * 3) as f32));
        assert_eq!(node.height, Val::Px((bmp.height * 3) as f32));
        let handle = world.get::<ImageNode>(e).expect("an image is attached").image.clone();
        let image = world.resource::<Assets<Image>>().get(&handle).expect("the image exists");
        assert_eq!(image.width() as usize, bmp.width * 3);
        // Some pixel is lit (alpha 255), and the corner outside the letters is clear.
        let data = image.data.as_ref().unwrap();
        assert!(data.chunks(4).any(|p| p[3] == 255));
    }

    #[test]
    fn changing_the_text_redraws_it() {
        let mut app = app();
        let e = app.world_mut().spawn(VerticalText::new(format!("{MA}"), 0, 1)).id();
        app.update();
        let h1 = match app.world().get::<Node>(e).unwrap().height {
            Val::Px(h) => h,
            _ => panic!(),
        };
        app.world_mut().get_mut::<VerticalText>(e).unwrap().text = format!("{MA}{O}{NGA}{NA}");
        app.update();
        let h2 = match app.world().get::<Node>(e).unwrap().height {
            Val::Px(h) => h,
            _ => panic!(),
        };
        assert!(h2 > h1, "{h2} should exceed {h1}");
    }
}
