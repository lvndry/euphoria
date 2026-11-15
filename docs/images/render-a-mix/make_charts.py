"""Writes the chart of docs/guides/render-a-mix.md as an SVG file next to this script.

Run it with any Python 3 from the repository root:

    python3 docs/images/render-a-mix/make_charts.py

The curves are the ones `euphoria set render` applies over a blend, from
crates/euphoria-core/src/set_render/timeline.rs: the incoming track fades in
over the first half of the window, the outgoing one fades out over the
second half, both with equal power, and the bass moves from one to the other
in one beat at the swap bar. Change that file and this one together.
"""

import math
from pathlib import Path
from xml.sax.saxutils import escape

OUTPUT_DIRECTORY = Path(__file__).resolve().parent

# The example on the page: a blend of 32 bars with the bass swap at bar 9.
BARS = 32
SWAP_BAR = 9
SAMPLES_PER_BAR = 16

STYLE = """
.surface { fill: #fcfcfb; stroke: rgba(11,11,11,0.10); }
.title { fill: #0b0b0b; font-size: 16px; font-weight: 600; }
.subtitle { fill: #52514e; font-size: 13px; }
.label { fill: #52514e; font-size: 13px; }
.tick { fill: #898781; font-size: 12px; font-variant-numeric: tabular-nums; }
.grid { stroke: #e1e0d9; stroke-width: 1; fill: none; }
.axis { stroke: #c3c2b7; stroke-width: 1; fill: none; }
.incoming { stroke: #2a78d6; stroke-width: 2.5; fill: none; stroke-linejoin: round; }
.outgoing { stroke: #c4610a; stroke-width: 2.5; fill: none; stroke-linejoin: round; }
.bass { stroke-dasharray: 5 4; stroke-width: 2; }
.incoming-text { fill: #2a78d6; font-size: 13px; }
.outgoing-text { fill: #c4610a; font-size: 13px; }
.marker { stroke: #898781; stroke-width: 1; stroke-dasharray: 2 3; fill: none; }
@media (prefers-color-scheme: dark) {
  .surface { fill: #1a1a19; stroke: rgba(255,255,255,0.10); }
  .title { fill: #f0efec; }
  .subtitle, .label { fill: #c3c2b7; }
  .grid { stroke: #2c2c2a; }
  .axis { stroke: #383835; }
  .incoming { stroke: #3987e5; }
  .outgoing { stroke: #e0812f; }
  .incoming-text { fill: #3987e5; }
  .outgoing-text { fill: #e0812f; }
  .marker { stroke: #a3a29c; }
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



def incoming_volume(bar):
    progress = min(max(bar / (BARS / 2), 0.0), 1.0)
    return math.sin(progress * math.pi / 2)


def outgoing_volume(bar):
    progress = min(max((bar - BARS / 2) / (BARS / 2), 0.0), 1.0)
    return math.cos(progress * math.pi / 2)


def incoming_bass(bar):
    return min(max((bar - (SWAP_BAR - 1)) / 0.25, 0.0), 1.0)


def blend_chart():
    width = 800
    height = 380
    plot_left = 64
    plot_right = 600
    plot_top = 100
    plot_bottom = 300
    to_x = scale(0, BARS, plot_left, plot_right)
    to_y = scale(0, 1, plot_bottom, plot_top)
    title = "How one track blends into the next"
    body = [
        text(24, 36, title, "title"),
        text(24, 58, f"A blend of {BARS} bars with the bass swap at bar {SWAP_BAR}.", "subtitle"),
    ]
    for level in (0, 0.5, 1):
        body.append(line(plot_left, to_y(level), plot_right, to_y(level), "grid"))
        body.append(text(plot_left - 10, to_y(level) + 4, f"{int(level * 100)}%", "tick", "end"))
    body.append(line(plot_left, plot_bottom, plot_right, plot_bottom, "axis"))
    for bar in (1, 9, 17, 25, 33):
        body.append(text(to_x(bar - 1), plot_bottom + 20, str(bar), "tick", "middle"))
    body.append(text((plot_left + plot_right) / 2, plot_bottom + 44, "Bar of the blend", "label", "middle"))
    body.append(line(to_x(SWAP_BAR - 1), plot_top - 8, to_x(SWAP_BAR - 1), plot_bottom, "marker"))
    body.append(text(to_x(SWAP_BAR - 1) + 6, plot_top - 12, "Bass swap", "label"))

    steps = [step / SAMPLES_PER_BAR for step in range(BARS * SAMPLES_PER_BAR + 1)]
    curves = [
        ("incoming", "", incoming_volume, "Incoming, level"),
        ("outgoing", "", outgoing_volume, "Outgoing, level"),
        ("incoming", " bass", incoming_bass, "Incoming, bass"),
        ("outgoing", " bass", lambda bar: 1 - incoming_bass(bar), "Outgoing, bass"),
    ]
    for index, (side, extra, curve, label) in enumerate(curves):
        body.append(polyline([(to_x(bar), to_y(curve(bar))) for bar in steps], side + extra))
        legend_y = 112 + index * 28
        legend_x = plot_right + 24
        body.append(line(legend_x, legend_y, legend_x + 28, legend_y, side + extra))
        body.append(text(legend_x + 36, legend_y + 4, label, side + "-text"))
    return document(width, height, title, DESCRIPTION, body)


DESCRIPTION = (
    "Line chart of level against the bars of a 32 bar blend. The incoming track rises from "
    "silence to full level over the first 16 bars while the outgoing one stays full, then the "
    "outgoing one falls to silence over the last 16. Their bass lines, dashed, cross at bar 9: "
    "the incoming bass comes in and the outgoing bass goes out within one beat, so the two "
    "basslines never play together."
)

CHARTS = {
    "blend-levels.svg": blend_chart,
}


def main():
    for file_name, build in CHARTS.items():
        (OUTPUT_DIRECTORY / file_name).write_text(build(), encoding="utf-8")


if __name__ == "__main__":
    main()
