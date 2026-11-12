# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Writes the charts of the beat grid and cue point sections of
docs/concepts/accuracy.md as SVG files next to this script.

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

# "Cue points", the chart: the energy curve of GiantSteps Key preview
# 3533956 at its detected tempo of 123.9 BPM, as (window centre in seconds,
# rating), and the cues Euphoria sets on it, as (seconds, name).
CURVE_POINTS = [
    (4, 3), (8, 2), (12, 3), (16, 7), (20, 8), (24, 8), (28, 8), (32, 8),
    (36, 8), (40, 8), (44, 8), (48, 8), (52, 8), (56, 8), (60, 8), (64, 8),
    (68, 8), (72, 8), (76, 4), (80, 1), (84, 1), (88, 1), (92, 4), (96, 7),
    (100, 7), (104, 7), (108, 7), (112, 8), (116, 8),
]
CURVE_STEP_SECONDS = 4
CUES = [(0.24, "Start"), (15.72, "Main"), (77.66, "Breakdown"), (93.15, "Drop")]

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


def minutes_and_seconds(seconds):
    return f"{seconds // 60}:{seconds % 60:02d}"


def cues_chart():
    width = 800
    height = 380
    plot_top = 116
    plot_bottom = 316
    plot_right = 740
    to_x = scale(0, 120, 64, plot_right)
    to_y = scale(1, 10, plot_bottom, plot_top)
    body = [
        text(24, 36, "Cues on a two minute preview", "title"),
        text(24, 58, "Where Euphoria sets cues, over the energy rating of the preview every 4 seconds.", "subtitle"),
    ]
    for rating_tick in (1, 4, 7, 10):
        body.append(line(to_x(0), to_y(rating_tick), to_x(120), to_y(rating_tick), "grid"))
        body.append(text(to_x(0) - 10, to_y(rating_tick) + 4, str(rating_tick), "tick", "end"))
    body.append(line(to_x(0), plot_bottom, to_x(120), plot_bottom, "axis"))
    for second_tick in range(0, 121, 20):
        body.append(text(to_x(second_tick), plot_bottom + 20, minutes_and_seconds(second_tick), "tick", "middle"))
    body.append(text(to_x(60), plot_bottom + 44, "Time in the preview", "label", "middle"))

    half_step = CURVE_STEP_SECONDS / 2
    steps = []
    for centre, rating in CURVE_POINTS:
        steps.append((to_x(centre - half_step), to_y(rating)))
        steps.append((to_x(centre + half_step), to_y(rating)))
    wash = [(steps[0][0], plot_bottom), *steps, (steps[-1][0], plot_bottom)]
    wash_points = " ".join(f"{number(x)},{number(y)}" for x, y in wash)
    body.append(f'<polygon points="{wash_points}" class="series-wash"/>')
    body.append(polyline(steps, "series-line"))
    for seconds, name in CUES:
        body.append(line(to_x(seconds), plot_top - 22, to_x(seconds), plot_bottom, "reference"))
        body.append(text(to_x(seconds) + 6, plot_top - 26, name, "value"))

    description = (
        "Step chart of the energy rating of a two minute GiantSteps preview at 123.9 BPM, "
        "with vertical lines at the four cues Euphoria sets. Start is at 0.2 seconds, where "
        "the rating is 2 to 3. Main is at 15.7 seconds, where the rating rises to 7 and then 8. "
        "Breakdown is at 77.7 seconds, where it falls to 4 and then 1. Drop is at 93.1 seconds, "
        "where it comes back to 7 and then 8."
    )
    return document(width, height, "Cues on a two minute preview", description, body)


CHARTS = {
    "confidence.svg": confidence_chart,
    "cues.svg": cues_chart,
}


def main():
    for file_name, build in CHARTS.items():
        (OUTPUT_DIRECTORY / file_name).write_text(build(), encoding="utf-8")


if __name__ == "__main__":
    main()
