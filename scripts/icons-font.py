#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["fonttools==4.68.0", "picosvg==0.23.0", "skia-pathops==0.9.2"]
# ///
# Rebuilds crates/citrine-gb-app/assets/fonts/CitrineIcons.ttf, the app's own
# icons, from the SVGs listed in crates/citrine-gb-app/assets/icons/icons.txt.
# Run it from the repository root after adding or changing an icon:
#   uv run scripts/icons-font.py
#
# Each SVG is drawn on Phosphor's 256 grid (stroke 16, round caps and joins)
# and lands at Phosphor's metrics, so the glyphs sit beside Phosphor Regular.
# The name table records an FNV-1a 64 checksum of the sources, which the
# app's tests recompute to catch a font that is older than its SVGs.
import pathlib
import sys

import pathops
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.cu2quPen import Cu2QuPen
from fontTools.pens.transformPen import TransformPen
from fontTools.pens.ttGlyphPen import TTGlyphPen
from picosvg.svg import SVG
from picosvg.svg_pathops import skia_path

UPM, ASCENT, DESCENT, ADVANCE = 1024, 960, -64, 1024
ROOT = pathlib.Path("crates/citrine-gb-app/assets")
ICONS = ROOT / "icons"
OUT = ROOT / "fonts" / "CitrineIcons.ttf"


def manifest():
    entries = []
    for line in (ICONS / "icons.txt").read_text().splitlines():
        line = line.strip()
        if line:
            point, name = line.split()
            entries.append((int(point, 16), name))
    return entries


def fnv1a64(entries):
    value = 0xCBF29CE484222325
    for _, name in entries:
        for byte in name.encode() + b"\0" + (ICONS / f"{name}.svg").read_bytes():
            value = ((value ^ byte) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return f"{value:016x}"


def outline(path):
    pico = SVG.parse(str(path)).topicosvg(allow_text=False)
    out = pathops.Path()
    for shape in pico.shapes():
        out = pathops.op(
            out, skia_path(shape.as_cmd_seq(), shape.fill_rule), pathops.PathOp.UNION
        )
    return out


def glyph(path):
    pen = TTGlyphPen(None)
    outline(path).draw(
        Cu2QuPen(TransformPen(pen, (4, 0, 0, -4, 0, ASCENT)), 1.0, reverse_direction=True)
    )
    built = pen.glyph()
    built.recalcBounds(None)
    return built


def main():
    entries = manifest()
    builder = FontBuilder(UPM, isTTF=True)
    builder.setupGlyphOrder([".notdef"] + [name for _, name in entries])
    builder.setupCharacterMap({point: name for point, name in entries})
    glyphs = {".notdef": TTGlyphPen(None).glyph()}
    metrics = {".notdef": (ADVANCE, 0)}
    for _, name in entries:
        glyphs[name] = glyph(ICONS / f"{name}.svg")
        metrics[name] = (ADVANCE, glyphs[name].xMin if glyphs[name].numberOfContours else 0)
    builder.setupGlyf(glyphs)
    builder.setupHorizontalMetrics(metrics)
    builder.setupHorizontalHeader(ascent=ASCENT, descent=DESCENT, lineGap=0)
    builder.setupOS2(
        sTypoAscender=ASCENT,
        sTypoDescender=DESCENT,
        sTypoLineGap=64,
        usWinAscent=ASCENT,
        usWinDescent=-DESCENT,
        fsType=0,
    )
    checksum = fnv1a64(entries)
    builder.setupNameTable(
        {
            "familyName": "Citrine Icons",
            "styleName": "Regular",
            "version": f"Version 1.0; sources {checksum}",
        }
    )
    builder.setupPost()
    builder.save(OUT)
    print(f"{OUT}: {len(entries)} icons, sources {checksum}")


if __name__ == "__main__":
    sys.exit(main())
