# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Writes the charts of docs/concepts/keys-and-camelot.md as SVG files next to
this script.

Run it with any Python 3 from the repository root:

    python3 docs/images/keys-and-camelot/make_charts.py

Every key and move comes from docs/concepts/keys-and-camelot.md, which
describes crates/euphoria-core/src/key.rs and
crates/euphoria-core/src/harmony/relation.rs. Change the page and the data
below together.
"""

import math
from pathlib import Path
from xml.sax.saxutils import escape

OUTPUT_DIRECTORY = Path(__file__).resolve().parent

# "Camelot codes", the table: the minor key (A) and the major key (B) at each
# number of the wheel, written short as on most Camelot wheels.
MINOR_KEYS = ["G#m", "Ebm", "Bbm", "Fm", "Cm", "Gm", "Dm", "Am", "Em", "Bm", "F#m", "C#m"]
MAJOR_KEYS = ["B", "F#", "Db", "Ab", "Eb", "Bb", "F", "C", "G", "D", "A", "E"]

# "Which keys mix well" and "Bolder moves", the column "From 8A (A minor)".
START_CODE = "8A"
MIXES_IN_KEY = [("8A", "same key"), ("7A", "a fifth down"), ("9A", "a fifth up"), ("8B", "relative major")]
BOLDER_MOVES = [("7B", "diagonal"), ("10A", "energy boost"), ("11B", "mood change"), ("3A", "semitone boost")]

STYLE = """
.surface { fill: #fcfcfb; stroke: rgba(11,11,11,0.10); }
.title { fill: #0b0b0b; font-size: 16px; font-weight: 600; }
.subtitle { fill: #52514e; font-size: 13px; }
.label { fill: #52514e; font-size: 13px; }
.value { fill: #0b0b0b; font-size: 13px; font-weight: 600; }
.code { font-size: 13px; font-weight: 600; }
.key { font-size: 11px; }
.cell { stroke: #fcfcfb; stroke-width: 2; }
.cell-plain { fill: #f0efec; }
.cell-start { fill: #0b0b0b; }
.cell-near { fill: #1c5cab; }
.cell-bold { fill: #86b6ef; }
.on-plain { fill: #52514e; }
.on-plain.code { fill: #0b0b0b; }
.on-start { fill: #fcfcfb; }
.on-near { fill: #ffffff; }
.on-bold { fill: #0b0b0b; }
@media (prefers-color-scheme: dark) {
  .surface { fill: #1a1a19; stroke: rgba(255,255,255,0.10); }
  .title, .value { fill: #f0efec; }
  .subtitle, .label { fill: #c3c2b7; }
  .cell { stroke: #1a1a19; }
  .cell-plain { fill: #2c2c2a; }
  .cell-start { fill: #f0efec; }
  .cell-near { fill: #6da7ec; }
  .cell-bold { fill: #184f95; }
  .on-plain { fill: #c3c2b7; }
  .on-plain.code { fill: #f0efec; }
  .on-start { fill: #1a1a19; }
  .on-near { fill: #0b0b0b; }
  .on-bold { fill: #f0efec; }
}
"""

FONT_STACK = 'system-ui, -apple-system, "Segoe UI", Helvetica, Arial, sans-serif'


def number(value):
    return f"{value:.2f}".rstrip("0").rstrip(".")


def text(x, y, content, css_class, anchor="start"):
    return (
        f'<text x="{number(x)}" y="{number(y)}" class="{css_class}" '
        f'text-anchor="{anchor}">{escape(content)}</text>'
    )


def document(width, height, title, description, body):
    return "\n".join(
        [
            f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" '
            f'viewBox="0 0 {width} {height}" role="img" '
            'aria-labelledby="chart-title chart-desc" '
            f"font-family='{FONT_STACK}'>",
            f'<title id="chart-title">{escape(title)}</title>',
            f'<desc id="chart-desc">{escape(description)}</desc>',
            f"<style>{STYLE}</style>",
            f'<rect x="0.5" y="0.5" width="{width - 1}" height="{height - 1}" rx="12" class="surface"/>',
            *body,
            "</svg>",
            "",
        ]
    )


def polar(centre_x, centre_y, radius, degrees_from_top):
    radians = math.radians(degrees_from_top)
    return centre_x + radius * math.sin(radians), centre_y - radius * math.cos(radians)


def ring_segment(centre_x, centre_y, inner_radius, outer_radius, start_degrees, end_degrees, css_class):
    outer_start = polar(centre_x, centre_y, outer_radius, start_degrees)
    outer_end = polar(centre_x, centre_y, outer_radius, end_degrees)
    inner_end = polar(centre_x, centre_y, inner_radius, end_degrees)
    inner_start = polar(centre_x, centre_y, inner_radius, start_degrees)
    return (
        f'<path class="cell {css_class}" d="M{number(outer_start[0])},{number(outer_start[1])} '
        f"A{number(outer_radius)},{number(outer_radius)} 0 0 1 {number(outer_end[0])},{number(outer_end[1])} "
        f"L{number(inner_end[0])},{number(inner_end[1])} "
        f"A{number(inner_radius)},{number(inner_radius)} 0 0 0 {number(inner_start[0])},{number(inner_start[1])} "
        'Z"/>'
    )


def legend_swatch(x, y, css_class):
    return f'<rect x="{number(x)}" y="{number(y - 11)}" width="14" height="14" rx="3" class="cell {css_class}"/>'


def camelot_wheel_chart():
    width = 800
    height = 520
    centre_x = 250
    centre_y = 290
    hole_radius = 72
    ring_boundary = 138
    outer_radius = 205
    near_codes = {code for code, _ in MIXES_IN_KEY if code != START_CODE}
    bold_codes = {code for code, _ in BOLDER_MOVES}

    def state_of(code):
        if code == START_CODE:
            return "start"
        if code in near_codes:
            return "near"
        if code in bold_codes:
            return "bold"
        return "plain"

    body = [
        text(24, 36, "The Camelot wheel, from 8A", "title"),
        text(24, 58, "Where a track in A minor can go next. Numbers step by a fifth clockwise and wrap from 12 to 1.", "subtitle"),
    ]
    rings = [
        ("A", MINOR_KEYS, hole_radius, ring_boundary),
        ("B", MAJOR_KEYS, ring_boundary, outer_radius),
    ]
    for letter, keys, inner_radius, outer_ring_radius in rings:
        middle_radius = (inner_radius + outer_ring_radius) / 2
        for wheel_number in range(1, 13):
            code = f"{wheel_number}{letter}"
            state = state_of(code)
            centre_degrees = (wheel_number % 12) * 30
            body.append(
                ring_segment(
                    centre_x, centre_y, inner_radius, outer_ring_radius,
                    centre_degrees - 15, centre_degrees + 15, f"cell-{state}",
                )
            )
            label_x, label_y = polar(centre_x, centre_y, middle_radius, centre_degrees)
            body.append(text(label_x, label_y - 1, code, f"code on-{state}", "middle"))
            body.append(text(label_x, label_y + 13, keys[wheel_number - 1], f"key on-{state}", "middle"))
    body.append(text(centre_x, centre_y - 4, "A inside", "label", "middle"))
    body.append(text(centre_x, centre_y + 14, "B outside", "label", "middle"))

    legend_x = 520
    legend_y = 110
    body.append(legend_swatch(legend_x, legend_y, "cell-start"))
    body.append(text(legend_x + 24, legend_y, "8A, the track playing", "value"))
    legend_y += 22
    body.append(text(legend_x + 24, legend_y, "Any track in 8A mixes too", "label"))

    legend_y += 44
    body.append(legend_swatch(legend_x, legend_y, "cell-near"))
    body.append(text(legend_x + 24, legend_y, "Mixes in key", "value"))
    for code, name in MIXES_IN_KEY:
        if code == START_CODE:
            continue
        legend_y += 22
        body.append(text(legend_x + 24, legend_y, f"{code}  {name}", "label"))

    legend_y += 44
    body.append(legend_swatch(legend_x, legend_y, "cell-bold"))
    body.append(text(legend_x + 24, legend_y, "Bolder moves", "value"))
    for code, name in BOLDER_MOVES:
        legend_y += 22
        body.append(text(legend_x + 24, legend_y, f"{code}  {name}", "label"))

    legend_y += 44
    body.append(text(legend_x, legend_y, "A is minor, B is major. Same number,", "label"))
    body.append(text(legend_x, legend_y + 18, "other letter: the relative key.", "label"))

    description = (
        "The Camelot wheel as two rings of twelve: minor keys, A, inside and major keys, B, outside, "
        "with 12 at the top and the numbers rising clockwise by a fifth. From 8A, A minor, the moves "
        "that mix in key are highlighted strongly: 8A itself, 7A D minor and 9A E minor a fifth away, "
        "and 8B C major, the relative major. The bolder moves are highlighted lightly: 7B F major, "
        "the diagonal; 10A B minor, the energy boost; 11B A major, the mood change; and 3A Bb minor, "
        "the semitone boost. The other keys are plain."
    )
    return document(width, height, "The Camelot wheel, from 8A", description, body)


CHARTS = {
    "camelot-wheel.svg": camelot_wheel_chart,
}


def main():
    for file_name, build in CHARTS.items():
        (OUTPUT_DIRECTORY / file_name).write_text(build(), encoding="utf-8")


if __name__ == "__main__":
    main()
