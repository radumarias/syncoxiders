"""Derive Oxfer Segment, Phosphor's display face, from DSEG14 Classic Italic.

Usage: python3 derive-oxfer-segment.py DSEG14Classic-Italic.ttf OxferSegment-Italic.ttf

The source is `fonts/DSEG14-Classic/DSEG14Classic-Italic.ttf` from the `dseg` npm
package, version 0.46.0 (sha256 bd65525be4282621f2b79f7e4344007f1a1f51f5311fee1d77ede77919825218).
Requires fontTools.

DSEG is a faithful 14-segment display, so a space is a fifth of a cell, a period is a
zero-width decimal point lit in the previous cell, and a cell fills the line from baseline to
ascender. That reads well on a clock and badly in sentences, so this derivative widens the
space, pads each line above and below, and adds the few punctuation marks the app's copy uses. The segment outlines themselves are unchanged. DSEG is a Reserved Font Name
under the SIL OFL 1.1, so the modified font is renamed.
"""

import sys

from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import TTFont

NAME = "Oxfer Segment"
CELL = 816
SPACE = 480
PAD = 160  # line padding above and below a cell, which spans 0..1000
DOT = 124  # the decimal point's width and height


def dots(font, centres_x, y):
    pen = TTGlyphPen(font.getGlyphSet())
    for x in centres_x:
        left, bottom = x - DOT // 2, y
        pen.moveTo((left, bottom))
        pen.lineTo((left, bottom + DOT))
        pen.lineTo((left + DOT, bottom + DOT))
        pen.lineTo((left + DOT, bottom))
        pen.closePath()
    return pen.glyph()


def main(src, dst):
    font = TTFont(src)
    glyf, hmtx = font["glyf"], font["hmtx"]
    hmtx["space"] = (SPACE, 0)
    hhea, os2 = font["hhea"], font["OS/2"]
    hhea.ascent = os2.sTypoAscender = os2.usWinAscent = 1000 + PAD
    hhea.descent = os2.sTypoDescender = -PAD
    os2.usWinDescent = PAD

    def add(name, glyph, advance):
        glyf[name] = glyph  # also appends the name to the glyph order
        glyph.recalcBounds(glyf)
        hmtx[name] = (advance, glyph.xMin if glyph.numberOfContours else 0)

    # "Connecting…" and "a · b" are everywhere in the app's copy.
    add("ellipsis", dots(font, [90, 240, 390], 0), SPACE)
    add("periodcentered", dots(font, [SPACE // 2], 500 - DOT // 2), SPACE)
    font.setGlyphOrder(glyf.glyphOrder)

    aliases = {
        0x2014: "hyphen",  # em dash
        0x2013: "hyphen",  # en dash
        0x2018: "quotesingle",
        0x2019: "quotesingle",
        0x201C: "quotedbl",
        0x201D: "quotedbl",
        0x2026: "ellipsis",
        0x00B7: "periodcentered",
    }
    for table in font["cmap"].tables:
        if table.isUnicode():
            table.cmap.update(aliases)

    name = font["name"]
    for record in list(name.names):
        if record.nameID in (1, 3, 4, 6, 16, 19):
            name.removeNames(nameID=record.nameID)
    name.setName(NAME, 1, 3, 1, 0x409)
    name.setName("Italic", 2, 3, 1, 0x409)
    name.setName(f"{NAME} Italic; derived from DSEG14 Classic Italic 0.46", 3, 3, 1, 0x409)
    name.setName(f"{NAME} Italic", 4, 3, 1, 0x409)
    name.setName("OxferSegment-Italic", 6, 3, 1, 0x409)
    name.setName(
        "Copyright (c) 2018, keshikan (http://www.keshikan.net), with Reserved Font Name "
        '"DSEG". Modified for Oxfer: wider space, added punctuation.',
        0,
        3,
        1,
        0x409,
    )
    # Mac-platform names are a duplicate set; the Windows ones above are what egui reads.
    name.names = [r for r in name.names if r.platformID == 3]
    font.save(dst)


if __name__ == "__main__":
    main(*sys.argv[1:])
