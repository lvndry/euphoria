"""Writes the figure of docs/concepts/plan-a-transition.md as an SVG file next to this script.

Run it with any Python 3 from the repository root:

    python3 docs/images/transition/make_figure.py

It reuses the helpers and the style of docs/images/energy/make_charts.py. The
two tracks are an invented example of a 32 bar window, chosen to show each kind
of clash once. They are not measurements.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "energy"))

import make_charts as charts  # noqa: E402

OUTPUT = Path(__file__).resolve().parent / "window.svg"

BARS = 32
A_VOCALS = range(0, 12)
A_BASS = range(0, 20)
B_VOCALS = range(8, 32)
B_BASS = range(4, 32)

EXTRA_STYLE = """
.absent { fill: #e1e0d9; }
.present { fill: #2a78d6; }
.vocal-clash { fill: #d6402a; }
.bass-clash { fill: #d69a2a; }
.clear { fill: #2f9e63; }
.marker { stroke: #0b0b0b; stroke-width: 1.5; stroke-dasharray: 4 3; fill: none; }
@media (prefers-color-scheme: dark) {
  .absent { fill: #2c2c2a; }
  .present { fill: #3987e5; }
  .vocal-clash { fill: #e5604b; }
  .bass-clash { fill: #e5ad4b; }
  .clear { fill: #4bc287; }
  .marker { stroke: #f0efec; }
}
"""


def lane(label, present, row_top, left, cell, css_when_present="present"):
    body = [charts.text(left - 12, row_top + 15, label, "label", "end")]
    for bar in range(BARS):
        css = css_when_present if bar in present else "absent"
        body.append(
            f'<rect x="{charts.number(left + bar * cell + 0.5)}" y="{row_top}" '
            f'width="{charts.number(cell - 1)}" height="22" rx="2" class="{css}"/>'
        )
    return body


def build():
    width = 800
    height = 380
    left = 130
    cell = (width - 40 - left) / BARS
    body = [
        charts.text(24, 36, "One 32 bar window, bar by bar", "title"),
        charts.text(24, 58, "An invented example: the lanes of both tracks and the clash they make.", "subtitle"),
    ]
    rows = [
        ("A vocals", A_VOCALS),
        ("A bass", A_BASS),
        ("B vocals", B_VOCALS),
        ("B bass", B_BASS),
    ]
    top = 90
    for index, (label, present) in enumerate(rows):
        body += lane(label, present, top + index * 34, left, cell)
    strip_top = top + len(rows) * 34 + 14
    body.append(charts.text(left - 12, strip_top + 15, "Clash", "label", "end"))
    swap_bar = max(A_BASS)
    for bar in range(BARS):
        if bar in A_VOCALS and bar in B_VOCALS:
            css = "vocal-clash"
        elif bar in A_BASS and bar in B_BASS:
            css = "bass-clash"
        else:
            css = "clear"
        body.append(
            f'<rect x="{charts.number(left + bar * cell + 0.5)}" y="{strip_top}" '
            f'width="{charts.number(cell - 1)}" height="22" rx="2" class="{css}"/>'
        )
    marker_x = left + (swap_bar + 1) * cell
    body.append(charts.line(marker_x, top - 10, marker_x, strip_top + 32, "marker"))
    body.append(charts.text(marker_x + 6, top - 14, "bass swap at bar 21", "value"))
    axis_top = strip_top + 52
    for bar in (1, 8, 16, 24, 32):
        body.append(charts.text(left + (bar - 0.5) * cell, axis_top, str(bar), "tick", "middle"))
    body.append(charts.text(left + BARS * cell / 2, axis_top + 22, "Bar of the window", "label", "middle"))
    legend_top = axis_top + 46
    for index, (css, name) in enumerate(
        [
            ("vocal-clash", "Both sing"),
            ("bass-clash", "Both play bass"),
            ("clear", "No clash"),
        ]
    ):
        x = left + index * 170
        body.append(f'<rect x="{x}" y="{legend_top - 12}" width="14" height="14" rx="2" class="{css}"/>')
        body.append(charts.text(x + 22, legend_top, name, "label"))

    description = (
        "Four lanes of a 32 bar window and the clash strip under them. Track A sings in "
        "bars 1 to 12 and plays bass in bars 1 to 20. Track B sings from bar 9 and plays bass "
        "from bar 5. Bars 9 to 12 clash on vocals, bars 5 to 20 clash on bass, and B's bass "
        "can stay in from bar 21, where the bass swap is marked."
    )
    document = charts.document(width, height, "One 32 bar window, bar by bar", description, body)
    return document.replace("</style>", EXTRA_STYLE + "</style>", 1)


if __name__ == "__main__":
    OUTPUT.write_text(build(), encoding="utf-8")
