#!/usr/bin/env python3
"""Builds src/cjk/{zh,zh_hant,ja,syllabics}.bin from GNU Unifont's precompiled .hex files.

Usage: tools/build_cjk_data.py <unifont-x.y.z/font/precompiled> [ArabicShaping.txt]

zh.bin is the base set (Simplified-leaning glyphs: unifont-X.hex, plus unifont_upper-X.hex above
the BMP). zh_hant.bin and ja.bin hold only the glyphs that differ from it (or that it lacks), taken
from unifont_t-X.hex and unifont_jp-X.hex. Each file is a zlib stream of records:
    u32 LE code point | u8 flags | 16 rows, `width` bytes each
flags: bits 0-3 width in 8-pixel columns (1 or 2); bit 7 combining mark (Mn/Me: drawn over the
previous letter, takes no room); bits 4-5 joining (1 right-joining, 2 dual-joining; from
ArabicShaping.txt, only for the scripts that join: Syriac, Mandaic, N'Ko).
syllabics.bin is Cherokee and the Canadian Aboriginal syllabics (Cree, Inuktitut, ...), same record format.
Only the CJK-related ranges below are kept; Latin, Hangul and the rest are the game's own.
Unifont is dual-licensed SIL OFL 1.1 / GPLv2+ with the font exception; see third_party/unifont/.
"""
import glob, struct, sys, unicodedata, zlib

RANGES = [
    # Symbols that Japanese and Chinese text uses (JIS X 0208, GB 2312, Big5): Greek, punctuation
    # (not the invisible format characters), letterlike, arrows, maths, boxes, shapes, circled numbers.
    (0x0391, 0x03C9), (0x2010, 0x2027), (0x2030, 0x205E), (0x2100, 0x218F), (0x2190, 0x21FF),
    (0x2200, 0x22FF), (0x2300, 0x23FF), (0x2460, 0x24FF), (0x2500, 0x259F), (0x25A0, 0x25FF),
    (0x2600, 0x26FF), (0x2700, 0x27BF),
    (0x2E80, 0x2FDF), (0x2FF0, 0x303F), (0x3040, 0x30FF), (0x3100, 0x312F), (0x3190, 0x31FF),
    (0x3200, 0x33FF), (0x3400, 0x4DBF), (0x4E00, 0x9FFF), (0xF900, 0xFAFF), (0xFE10, 0xFE1F),
    (0xFE30, 0xFE6F), (0xFF00, 0xFFEF), (0x1B000, 0x1B2FF), (0x1F200, 0x1F2FF),
    (0x20000, 0x2EE5D), (0x2F800, 0x2FA1F), (0x30000, 0x323AF),
]

# Other scripts, from one blob (`scripts.bin`): Greek (the parts the game's own font lacks), Armenian,
# Georgian, Syriac, Mandaic, Coptic, Ethiopic, Tifinagh (and neo-Tifinagh), N'Ko, Vai.
SCRIPTS = [
    (0x0370, 0x03FF), (0x1F00, 0x1FFF), (0x0530, 0x058F), (0xFB13, 0xFB17), (0x10A0, 0x10FF),
    (0x1C90, 0x1CBF), (0x2D00, 0x2D2F), (0x0700, 0x074F), (0x0860, 0x086F), (0x0840, 0x085F),
    (0x2C80, 0x2CFF), (0x1200, 0x139F), (0x2D80, 0x2DDF), (0xAB00, 0xAB2F), (0x1E7E0, 0x1E7FF),
    (0x2D30, 0x2D7F), (0x07C0, 0x07FF), (0xA500, 0xA63F), (0x102E0, 0x102FF),
    # Mongolian, Todo, Manchu, Sibe (one block) and its relatives: Phags-pa, Soyombo, Zanabazar Square.
    (0x1800, 0x18AF), (0x11660, 0x1167F), (0xA840, 0xA87F), (0x11A50, 0x11AAF), (0x11A00, 0x11A4F),
    # South Asia: Devanagari (Hindi, Nepali, Marathi), Gujarati, Gurmukhi, Bengali (and Assamese), Tamil, Kannada, Malayalam, Sinhala, Telugu, Odia.
    (0x0900, 0x097F), (0xA8E0, 0xA8FF), (0x0980, 0x09FF), (0x0A00, 0x0A7F), (0x0A80, 0x0AFF), (0x0B00, 0x0B7F), (0x0B80, 0x0BFF),
    (0x0C00, 0x0C7F), (0x0C80, 0x0CFF), (0x0D00, 0x0D7F), (0x0D80, 0x0DFF), (0x111E0, 0x111FF),
    # Tibetan, Thaana, Ol Chiki, Burmese (three blocks).
    (0x0F00, 0x0FFF), (0x0780, 0x07BF), (0x1C50, 0x1C7F), (0x1000, 0x109F), (0xA9E0, 0xA9FF), (0xAA60, 0xAA7F),
    # East and Southeast Asia: Nushu, Yi, Miao (Pollard), Lisu, Thai, Lao, Khmer.
    (0x1B170, 0x1B2FF), (0xA000, 0xA4CF), (0x16F00, 0x16F9F), (0xA4D0, 0xA4FF), (0x0E00, 0x0E7F),
    (0x0E80, 0x0EFF), (0x1780, 0x17FF), (0x19E0, 0x19FF),
    # The Philippines and Indonesia: Tagalog (Baybayin), Hanunoo, Buhid, Tagbanwa, Buginese (Lontara),
    # Balinese, Javanese, Sundanese, Batak.
    (0x1700, 0x177F), (0x1A00, 0x1A1F), (0x1B00, 0x1B7F), (0xA980, 0xA9DF), (0x1B80, 0x1BBF),
    (0x1CC0, 0x1CCF), (0x1BC0, 0x1BFF),
    # Zhuyin tone marks (the spacing modifiers Bopomofo is written with).
    (0x02C7, 0x02C7), (0x02C9, 0x02CB), (0x02D9, 0x02D9),
]

JOINING = {}

def load_joining(path):
    for line in open(path, encoding='utf8'):
        if line.startswith('#') or not line.strip():
            continue
        cp, _, kind = [x.strip() for x in line.split(';')[:3]]
        JOINING[int(cp, 16)] = {'R': 1, 'D': 2}.get(kind, 0)

SYLLABICS = [(0x13A0, 0x13FF), (0xAB70, 0xABBF), (0x1400, 0x167F), (0x18B0, 0x18FF)]

def wanted(c, ranges=None):
    return any(a <= c <= b for a, b in (ranges or RANGES))

def load(path, ranges=None):
    out = {}
    for line in open(path):
        if ':' not in line:
            continue
        cp, bits = line.strip().split(':')
        c = int(cp, 16)
        if wanted(c, ranges):
            out[c] = bits
    return out

def find(d, pattern):
    (p,) = glob.glob(f"{d}/{pattern}")
    return p

def write(path, glyphs):
    blob = bytearray()
    for c in sorted(glyphs):
        bits = glyphs[c]
        w = len(bits) // 32
        assert w in (1, 2) and len(bits) == 32 * w, (hex(c), len(bits))
        flags = w
        if unicodedata.category(chr(c)) in ('Mn', 'Me'):
            flags |= 0x80
        flags |= JOINING.get(c, 0) << 4
        blob += struct.pack('<IB', c, flags) + bytes.fromhex(bits)
    open(path, 'wb').write(zlib.compress(bytes(blob), 9))
    print(path, len(glyphs), 'glyphs', len(blob), 'raw ->', len(zlib.compress(bytes(blob), 9)), 'bytes')

d = sys.argv[1]
if len(sys.argv) > 2:
    load_joining(sys.argv[2])
base = load(find(d, 'unifont-[0-9]*.hex'))
upper = load(find(d, 'unifont_upper-[0-9]*.hex'))
for c, b in upper.items():
    base.setdefault(c, b)
write('src/cjk/zh.bin', base)
for name, pat in (('zh_hant', 'unifont_t-[0-9]*.hex'), ('ja', 'unifont_jp-[0-9]*.hex')):
    var = load(find(d, pat))
    diff = {c: b for c, b in var.items() if base.get(c) != b}
    write(f'src/cjk/{name}.bin', diff)
write('src/cjk/syllabics.bin', load(find(d, 'unifont-[0-9]*.hex'), SYLLABICS))
# Joining only applies to the three joining scripts here (Arabic and the rest use the game's own font).
scripts = load(find(d, 'unifont-[0-9]*.hex'), SCRIPTS)
for c, b in load(find(d, 'unifont_upper-[0-9]*.hex'), SCRIPTS).items():
    scripts.setdefault(c, b)
write('src/cjk/scripts.bin', scripts)
