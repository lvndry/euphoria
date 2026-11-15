# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Writes the cue colour scale of docs/concepts/accuracy.md#cue-points as an
SVG file next to this script.

Run it with any Python 3 from the repository root:

    python3 docs/images/cue-colours/make_charts.py

The colours come from the code: ENERGY_CUE_COLOURS in
crates/euphoria-core/src/cue_colours.rs, ENERGY_PAD_COLOURS in
crates/euphoria-core/src/export/engine.rs and SERATO_ENERGY_CUE_COLOURS in
crates/euphoria-core/src/tags/serato.rs. Change the code and the data below
together.
"""

from pathlib import Path
from xml.sax.saxutils import escape

OUTPUT_DIRECTORY = Path(__file__).resolve().parent

STEPS = ["1 to 2", "3 to 4", "5 to 6", "7 to 8", "9 to 10"]
EUPHORIA = ["#3A6FF7", "#16B4C8", "#34C46B", "#F2A62E", "#F04522"]
ENGINE_DJ = ["#1571E2", "#20C6C6", "#20C670", "#EF8130", "#CE3239"]
SERATO = ["#0000CC", "#00CCCC", "#00CC00", "#CC8800", "#CC0000"]
ROWS = [
    ("Euphoria, rekordbox, VirtualDJ", EUPHORIA),
    ("Engine DJ", ENGINE_DJ),
    ("Serato", SERATO),
]

STYLE = """
.surface { fill: #fcfcfb; stroke: rgba(11,11,11,0.10); }
.title { fill: #0b0b0b; font-size: 16px; font-weight: 600; }
.subtitle { fill: #52514e; font-size: 13px; }
.label { fill: #52514e; font-size: 13px; }
.tick { fill: #898781; font-size: 12px; font-variant-numeric: tabular-nums; }
.swatch { stroke: rgba(11,11,11,0.08); stroke-width: 1; }
@media (prefers-color-scheme: dark) {
  .surface { fill: #1a1a19; stroke: rgba(255,255,255,0.10); }
  .title { fill: #f0efec; }
  .subtitle, .label { fill: #c3c2b7; }
  .swatch { stroke: rgba(255,255,255,0.10); }
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


def scale_chart():
    width = 800
    left = 250
    right = 776
    gap = 8
    swatch_width = (right - left - gap * (len(STEPS) - 1)) / len(STEPS)
    swatch_height = 32
    first_row = 112
    row_height = 46
    height = first_row + row_height * len(ROWS) + 16
    title = "Cue colours by the energy of the section"
    body = [
        text(24, 36, title, "title"),
        text(24, 58, "Each cue analysis placed takes the colour of the energy from it to the next cue.", "subtitle"),
        text(left - 14, 94, "Energy", "label", "end"),
    ]
    for index, step in enumerate(STEPS):
        x = left + index * (swatch_width + gap)
        body.append(text(x + swatch_width / 2, 94, step, "tick", "middle"))
    for row, (name, colours) in enumerate(ROWS):
        top = first_row + row * row_height
        body.append(text(left - 14, top + 21, name, "label", "end"))
        for index, colour in enumerate(colours):
            x = left + index * (swatch_width + gap)
            body.append(
                f'<rect x="{number(x)}" y="{top}" width="{number(swatch_width)}" '
                f'height="{swatch_height}" rx="6" fill="{colour}" class="swatch"/>'
            )
    description = (
        "Five colour steps from cool to warm. Energy 1 and 2 are blue, 3 and 4 teal, "
        "5 and 6 green, 7 and 8 amber and 9 and 10 red. rekordbox and VirtualDJ get "
        "Euphoria's own colours, while Engine DJ and Serato get the nearest of their "
        "own pad colours: blue, cyan, green, orange and red. Traktor colours a cue by "
        "its type, so it gets none."
    )
    return document(width, height, title, description, body)


def main():
    (OUTPUT_DIRECTORY / "scale.svg").write_text(scale_chart(), encoding="utf-8")


if __name__ == "__main__":
    main()
