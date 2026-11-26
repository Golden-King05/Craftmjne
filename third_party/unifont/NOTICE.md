# GNU Unifont glyph data

The glyphs for Chinese, Japanese, Cherokee, the Canadian syllabics and the scripts in `src/scripts.rs`
(Armenian, Georgian, Syriac, Mandaic, Coptic, Ethiopic, Tifinagh, N'Ko, Vai, Greek Extended, Mongolian, the
Indic and Southeast Asian scripts, Tibetan, Thaana, Ol Chiki, Burmese, Yi, Miao, Nushu and more) - the
`src/cjk/*.bin` files - are bitmaps taken from **GNU Unifont 18.0.01** (<https://unifoundry.com/unifont/>),
converted by `tools/build_cjk_data.py` and turned into fonts in memory at run time (`src/cjk.rs`,
`src/syllabics.rs`, `src/scripts.rs`). Each glyph also carries two flags taken from the Unicode Character
Database (`UnicodeData.txt` category and `ArabicShaping.txt` joining type, Unicode License v3). Unifont's CJK ideographs descend from the WenQuanYi bitmap font,
and its Japanese forms from the Public Domain jiskan16 and related sources.

    Copyright (C) 1998-2026 Roman Czyborra, Paul Hardy, Qianqian Fang, Andrew Miller,
    Johnnie Weaver, David Corbett, Ælla Chiana Moskopp, Rebecca Bettencourt, Minseo Lee,
    Ho-Seok Ee, et al.

Unifont's glyphs are dual-licensed under the **SIL Open Font License 1.1** and under the GNU GPL
version 2 or later with the GNU Font Embedding Exception. This project uses them under the
SIL OFL 1.1; the licence text is in `OFL-1.1.txt` beside this file.

The font built from them is named "Craftmjne CJK" - not "Unifont", which the OFL reserves.
