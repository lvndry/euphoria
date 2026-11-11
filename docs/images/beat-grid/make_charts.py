# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Writes the charts of the beat grid section of docs/concepts/accuracy.md as
SVG files next to this script.

Run it with any Python 3 from the repository root:

    python3 docs/images/beat-grid/make_charts.py

Every number comes from docs/concepts/accuracy.md. Change the page and the
data below together.
"""

from pathlib import Path
from xml.sax.saxutils import escape

OUTPUT_DIRECTORY = Path(__file__).resolve().parent

# "Beat grid", the confidence table: for each level, the excerpts of the held
# out half rated at it and the share of them whose grid is right, with the
# octave of the tempo taken from the annotation.
HELD_OUT_BY_CONFIDENCE = [("High", 36, 97.2), ("Medium", 22, 100.0), ("Low", 42, 64.3)]
HELD_OUT_GRID_RIGHT = 84.0

STYLE = """
.surface { fill: #fcfcfb; stroke: rgba(11,11,11,0.10); }
.title { fill: #0b0b0b; font-size: 16px; font-weight: 600; }
.subtitle { fill: #52514e; font-size: 13px; }
.label { fill: #52514e; font-size: 13px; }
.value { fill: #0b0b0b; font-size: 13px; font-weight: 600; }
.tick { fill: #898781; font-size: 12px; font-variant-numeric: tabular-nums; }
.grid { stroke: #e1e0d9; stroke-width: 1; fill: none; }
.axis { stroke: #c3c2b7; stroke-width: 1; fill: none; }
.reference { stroke: #898781; stroke-width: 1.5; fill: none; }
.series-line { stroke: #2a78d6; stroke-width: 2; fill: none; stroke-linejoin: round; stroke-linecap: round; }
.series-fill { fill: #2a78d6; }
.series-wash { fill: #2a78d6; fill-opacity: 0.10; }
.ring { stroke: #fcfcfb; stroke-width: 2; }
@media (prefers-color-scheme: dark) {
  .surface { fill: #1a1a19; stroke: rgba(255,255,255,0.10); }
  .title, .value { fill: #f0efec; }
  .subtitle, .label { fill: #c3c2b7; }
  .grid { stroke: #2c2c2a; }
  .axis { stroke: #383835; }
  .series-line { stroke: #3987e5; }
  .series-fill, .series-wash { fill: #3987e5; }
  .ring { stroke: #1a1a19; }
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


def line(x1, y1, x2, y2, css_class):
    return (
        f'<line x1="{number(x1)}" y1="{number(y1)}" x2="{number(x2)}" '
        f'y2="{number(y2)}" class="{css_class}"/>'
    )


def polyline(points, css_class):
    joined = " ".join(f"{number(x)},{number(y)}" for x, y in points)
    return f'<polyline points="{joined}" class="{css_class}"/>'


def dot(x, y, radius=5):
    return (
        f'<circle cx="{number(x)}" cy="{number(y)}" r="{radius}" '
        'class="series-fill ring"/>'
    )


def bar_path(left, top, width, height, round_end):
    """A bar with a 4px rounded data end and a square baseline end."""
    radius = min(4, width / 2, height / 2)
    right = left + width
    bottom = top + height
    if round_end == "top":
        return (
            f'<path class="series-fill" d="M{number(left)},{number(bottom)} '
            f"V{number(top + radius)} Q{number(left)},{number(top)} {number(left + radius)},{number(top)} "
            f"H{number(right - radius)} Q{number(right)},{number(top)} {number(right)},{number(top + radius)} "
            f'V{number(bottom)} Z"/>'
        )
    return (
        f'<path class="series-fill" d="M{number(left)},{number(top)} '
        f"H{number(right - radius)} Q{number(right)},{number(top)} {number(right)},{number(top + radius)} "
        f"V{number(bottom - radius)} Q{number(right)},{number(bottom)} {number(right - radius)},{number(bottom)} "
        f'H{number(left)} Z"/>'
    )


def scale(domain_low, domain_high, range_low, range_high):
    def mapped(value):
        fraction = (value - domain_low) / (domain_high - domain_low)
        return range_low + fraction * (range_high - range_low)

    return mapped


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


def confidence_chart():
    width = 800
    row_height = 44
    chart_top = 96
    chart_bottom = chart_top + row_height * len(HELD_OUT_BY_CONFIDENCE)
    height = chart_bottom + 64
    to_x = scale(0, 100, 150, 700)
    body = [
        text(24, 36, "How often the beat grid is right at each confidence", "title"),
        text(24, 58, "100 held out GTZAN disco and hip hop excerpts. High and medium grids are almost always right.", "subtitle"),
    ]
    for share_tick in (0, 25, 50, 75, 100):
        body.append(line(to_x(share_tick), chart_top, to_x(share_tick), chart_bottom, "grid" if share_tick else "axis"))
        body.append(text(to_x(share_tick), chart_bottom + 20, f"{share_tick}%", "tick", "middle"))
    body.append(line(to_x(HELD_OUT_GRID_RIGHT), chart_top - 8, to_x(HELD_OUT_GRID_RIGHT), chart_bottom, "reference"))
    body.append(text(to_x(HELD_OUT_GRID_RIGHT), chart_top - 14, f"All excerpts {HELD_OUT_GRID_RIGHT:.0f}%", "tick", "middle"))
    body.append(text(to_x(50), chart_bottom + 44, "Share of excerpts whose grid is right", "label", "middle"))
    for index, (level, excerpts, right) in enumerate(HELD_OUT_BY_CONFIDENCE):
        row_middle = chart_top + row_height * index + row_height / 2
        body.append(text(to_x(0) - 14, row_middle + 4.5, f"{level}, {excerpts}", "label", "end"))
        body.append(bar_path(to_x(0), row_middle - 10, to_x(right) - to_x(0), 20, "right"))
        body.append(text(to_x(right) + 10, row_middle + 4.5, f"{right:.1f}%", "value"))

    description = (
        "Horizontal bars of how often the beat grid is right on the 100 held out "
        "GTZAN disco and hip hop excerpts, by confidence: 97.2% of the 36 rated high, "
        "100% of the 22 rated medium and 64.3% of the 42 rated low, against 84% of all "
        "excerpts."
    )
    return document(width, height, "How often the beat grid is right at each confidence", description, body)


CHARTS = {
    "confidence.svg": confidence_chart,
}


def main():
    for file_name, build in CHARTS.items():
        (OUTPUT_DIRECTORY / file_name).write_text(build(), encoding="utf-8")


if __name__ == "__main__":
    main()
