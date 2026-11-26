# GNU Unifont glyph data

The Chinese and Japanese text glyphs (`src/cjk/*.bin`) are bitmaps taken from **GNU Unifont 18.0.01**
(<https://unifoundry.com/unifont/>), converted by `tools/build_cjk_data.py` and turned into a font in
memory at run time (`src/cjk.rs`). Unifont's CJK ideographs descend from the WenQuanYi bitmap font,
and its Japanese forms from the Public Domain jiskan16 and related sources.

    Copyright (C) 1998-2026 Roman Czyborra, Paul Hardy, Qianqian Fang, Andrew Miller,
    Johnnie Weaver, David Corbett, Ælla Chiana Moskopp, Rebecca Bettencourt, Minseo Lee,
    Ho-Seok Ee, et al.

Unifont's glyphs are dual-licensed under the **SIL Open Font License 1.1** and under the GNU GPL
version 2 or later with the GNU Font Embedding Exception. This project uses them under the
SIL OFL 1.1; the licence text is in `OFL-1.1.txt` beside this file.

The font built from them is named "Craftmjne CJK" - not "Unifont", which the OFL reserves.
