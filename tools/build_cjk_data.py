#!/usr/bin/env python3
"""Builds src/cjk/{zh,zh_hant,ja,syllabics}.bin from GNU Unifont's precompiled .hex files.

Usage: tools/build_cjk_data.py <unifont-x.y.z/font/precompiled>

zh.bin is the base set (Simplified-leaning glyphs: unifont-X.hex, plus unifont_upper-X.hex above
the BMP). zh_hant.bin and ja.bin hold only the glyphs that differ from it (or that it lacks), taken
from unifont_t-X.hex and unifont_jp-X.hex. Each file is a zlib stream of records:
    u32 LE code point | u8 width in 8-pixel columns (1 or 2) | 16 rows, `width` bytes each
syllabics.bin is Cherokee and the Canadian Aboriginal syllabics (Cree, Inuktitut, ...), same record format.
Only the CJK-related ranges below are kept; Latin, Hangul and the rest are the game's own.
Unifont is dual-licensed SIL OFL 1.1 / GPLv2+ with the font exception; see third_party/unifont/.
"""
import glob, struct, sys, zlib

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
        blob += struct.pack('<IB', c, w) + bytes.fromhex(bits)
    open(path, 'wb').write(zlib.compress(bytes(blob), 9))
    print(path, len(glyphs), 'glyphs', len(blob), 'raw ->', len(zlib.compress(bytes(blob), 9)), 'bytes')

d = sys.argv[1]
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
