//! Shared inline color-marker syntax for chat text: `~(#RRGGBB)~` toggles a
//! colored run on or off. Used two ways through the exact same mechanism -
//! this is the "just use that same text chat format" reuse: a player typing
//! `~(#ff0000)~ danger! ~(#ff0000)~` in chat gets `danger!` rendered red,
//! and `/texture-report` (`commands.rs`) builds its green/yellow/red counts
//! with [`colorize`] and gets colored chat output for free - no separate
//! rendering path for system-generated vs. player-typed color.
//!
//! **Toggle, not a matched pair.** Any well-formed marker flips between
//! "not currently colored" and "colored with this marker's hex" - the
//! *closing* marker's own hex digits are read but discarded, only its shape
//! matters. This avoids requiring a typed closer to exactly repeat the
//! opener's hex (a player mistyping one digit would otherwise silently
//! leave the rest of their message uncolored instead of closing the span)
//! while still reading naturally as `~(#hex)~ text ~(#hex)~`.

use bevy::prelude::Color;

#[derive(Clone, Debug, PartialEq)]
pub struct ColoredSegment {
    pub text: String,
    pub color: Option<Color>,
}

/// Builds one marker-wrapped segment, e.g. `colorize("12 working", "00ff00")`
/// -> `"~(#00ff00)~12 working~(#00ff00)~"`. `hex` must be 6 hex digits -
/// callers only ever pass fixed string literals, so this doesn't validate.
pub fn colorize(text: &str, hex: &str) -> String {
    format!("~(#{hex})~{text}~(#{hex})~")
}

/// Splits `input` into plain-vs-colored runs per the `~(#hex)~` toggle
/// syntax described in the module docs. Text with no (valid) markers at all
/// comes back as a single `None`-colored segment.
pub fn parse_colored_segments(input: &str) -> Vec<ColoredSegment> {
    let mut segments = Vec::new();
    let mut rest = input;
    let mut current: Option<Color> = None;
    let mut buf = String::new();

    while let Some((start, end, hex)) = find_marker(rest) {
        buf.push_str(&rest[..start]);
        if !buf.is_empty() {
            segments.push(ColoredSegment { text: std::mem::take(&mut buf), color: current });
        }
        current = match current {
            None => parse_hex(&hex),
            Some(_) => None,
        };
        rest = &rest[end..];
    }
    buf.push_str(rest);
    if !buf.is_empty() {
        segments.push(ColoredSegment { text: buf, color: current });
    }
    if segments.is_empty() {
        segments.push(ColoredSegment { text: String::new(), color: None });
    }
    segments
}

/// Finds the next well-formed `~(#XXXXXX)~` marker in `s`, returning its
/// start/end byte offsets and the 6 hex digits inside. Retries past a `~(#`
/// that isn't followed by exactly 6 hex digits and `)~`, so stray text
/// containing a literal `~(#` doesn't need escaping - it just won't parse
/// as a marker.
fn find_marker(s: &str) -> Option<(usize, usize, String)> {
    let mut from = 0;
    while let Some(rel) = s[from..].find("~(#") {
        let start = from + rel;
        let hex_start = start + 3;
        if let Some(hex) = s.get(hex_start..hex_start + 6) {
            if hex.bytes().all(|b| b.is_ascii_hexdigit()) && s.get(hex_start + 6..hex_start + 8) == Some(")~") {
                return Some((start, hex_start + 8, hex.to_string()));
            }
        }
        from = start + 3;
    }
    None
}

fn parse_hex(hex: &str) -> Option<Color> {
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color::srgb_u8(r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_with_no_markers_is_one_uncolored_segment() {
        let segs = parse_colored_segments("hello world");
        assert_eq!(segs, vec![ColoredSegment { text: "hello world".into(), color: None }]);
    }

    #[test]
    fn a_marker_pair_colors_only_the_text_between_them() {
        let segs = parse_colored_segments("before ~(#ff0000)~red~(#ff0000)~ after");
        assert_eq!(segs.len(), 3);
        assert_eq!(segs[0], ColoredSegment { text: "before ".into(), color: None });
        assert_eq!(segs[1].text, "red");
        assert_eq!(segs[1].color, Some(Color::srgb_u8(0xff, 0, 0)));
        assert_eq!(segs[2], ColoredSegment { text: " after".into(), color: None });
    }

    #[test]
    fn the_closing_markers_own_hex_is_ignored() {
        // Mistyped closer (00ff00 instead of ff0000) still closes the span -
        // only the shape of the marker matters, not a matching hex.
        let segs = parse_colored_segments("~(#ff0000)~red~(#00ff00)~plain");
        assert_eq!(segs[0].color, Some(Color::srgb_u8(0xff, 0, 0)));
        assert_eq!(segs[1], ColoredSegment { text: "plain".into(), color: None });
    }

    #[test]
    fn an_unclosed_marker_colors_everything_to_the_end() {
        let segs = parse_colored_segments("plain ~(#00ff00)~green to the end");
        assert_eq!(segs[0], ColoredSegment { text: "plain ".into(), color: None });
        assert_eq!(segs[1].text, "green to the end");
        assert_eq!(segs[1].color, Some(Color::srgb_u8(0, 0xff, 0)));
    }

    #[test]
    fn malformed_markers_are_left_as_literal_text() {
        for text in ["~(#zzzzzz)~not hex", "~(#fff)~too short", "~(#ff0000)not closed"] {
            let segs = parse_colored_segments(text);
            assert_eq!(segs, vec![ColoredSegment { text: text.into(), color: None }], "{text}");
        }
    }

    #[test]
    fn colorize_wraps_text_in_matching_open_and_close_markers() {
        assert_eq!(colorize("hi", "ff00ff"), "~(#ff00ff)~hi~(#ff00ff)~");
        let segs = parse_colored_segments(&colorize("hi", "ff00ff"));
        assert_eq!(segs, vec![ColoredSegment { text: "hi".into(), color: Some(Color::srgb_u8(0xff, 0, 0xff)) }]);
    }

    #[test]
    fn empty_input_produces_one_empty_uncolored_segment() {
        assert_eq!(parse_colored_segments(""), vec![ColoredSegment { text: String::new(), color: None }]);
    }

    #[test]
    fn multiple_marker_pairs_toggle_independently() {
        let segs = parse_colored_segments("~(#ff0000)~a~(#ff0000)~b~(#00ff00)~c~(#00ff00)~");
        assert_eq!(segs.len(), 3);
        assert_eq!((segs[0].text.as_str(), segs[0].color), ("a", Some(Color::srgb_u8(0xff, 0, 0))));
        assert_eq!((segs[1].text.as_str(), segs[1].color), ("b", None));
        assert_eq!((segs[2].text.as_str(), segs[2].color), ("c", Some(Color::srgb_u8(0, 0xff, 0))));
    }
}
