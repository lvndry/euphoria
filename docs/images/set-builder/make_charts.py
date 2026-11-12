"""Writes the charts of docs/guides/build-a-set.md as SVG files next to this script.

Run it with any Python 3 from the repository root:

    python3 docs/images/set-builder/make_charts.py

Every number comes from docs/guides/build-a-set.md, which says how each set
was built. Change the page and the data below together.
"""

from pathlib import Path
from xml.sax.saxutils import escape

OUTPUT_DIRECTORY = Path(__file__).resolve().parent

# "Planned and actual energy": the curve given with --curve, and for each
# track of the set the energy the curve asks for and the energy of the
# track, from the JSON output of the command on that page.
CURVE_LEVELS = [4, 6, 8, 9, 7, 5]
TARGET_ENERGY = []
ACTUAL_ENERGY = []

# "When the curve cannot be met": the same pool with a curve that peaks at
# 10, which no track of the pool reaches.
UNREACHABLE_CURVE_LEVELS = [4, 7, 10, 10, 6]
UNREACHABLE_TARGET_ENERGY = []
UNREACHABLE_ACTUAL_ENERGY = []
UNREACHABLE_OFF_TARGET = []

STYLE = """
.surface { fill: #fcfcfb; stroke: rgba(11,11,11,0.10); }
.title { fill: #0b0b0b; font-size: 16px; font-weight: 600; }
.subtitle { fill: #52514e; font-size: 13px; }
.label { fill: #52514e; font-size: 13px; }
.tick { fill: #898781; font-size: 12px; font-variant-numeric: tabular-nums; }
.grid { stroke: #e1e0d9; stroke-width: 1; fill: none; }
.axis { stroke: #c3c2b7; stroke-width: 1; fill: none; }
.planned { stroke: #898781; stroke-width: 2; stroke-dasharray: 6 5; fill: none; stroke-linejoin: round; }
.series-line { stroke: #2a78d6; stroke-width: 2; fill: none; stroke-linejoin: round; stroke-linecap: round; }
.series-fill { fill: #2a78d6; }
.miss-fill { fill: #c4610a; }
.miss-line { stroke: #c4610a; stroke-width: 1.5; fill: none; }
.ring { stroke: #fcfcfb; stroke-width: 2; }
@media (prefers-color-scheme: dark) {
  .surface { fill: #1a1a19; stroke: rgba(255,255,255,0.10); }
  .title { fill: #f0efec; }
  .subtitle, .label { fill: #c3c2b7; }
  .grid { stroke: #2c2c2a; }
  .axis { stroke: #383835; }
  .planned { stroke: #a3a29c; }
  .series-line { stroke: #3987e5; }
  .series-fill { fill: #3987e5; }
  .miss-fill { fill: #e0812f; }
  .miss-line { stroke: #e0812f; }
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


def dot(x, y, css_class, radius=5):
    return (
        f'<circle cx="{number(x)}" cy="{number(y)}" r="{radius}" '
        f'class="{css_class} ring"/>'
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


def curve_chart(title, subtitle, levels, actual, off_target, description):
    width = 800
    height = 380
    plot_left = 64
    plot_right = 640
    plot_top = 100
    plot_bottom = 300
    count = len(actual)
    to_x = scale(1, count, plot_left, plot_right)
    to_y = scale(1, 10, plot_bottom, plot_top)
    body = [text(24, 36, title, "title"), text(24, 58, subtitle, "subtitle")]
    for energy_tick in (1, 4, 7, 10):
        body.append(line(plot_left, to_y(energy_tick), plot_right, to_y(energy_tick), "grid"))
        body.append(text(plot_left - 10, to_y(energy_tick) + 4, str(energy_tick), "tick", "end"))
    body.append(line(plot_left, plot_bottom, plot_right, plot_bottom, "axis"))
    for position in sorted({1, *range(5, count + 1, 5), count}):
        body.append(text(to_x(position), plot_bottom + 20, str(position), "tick", "middle"))
    body.append(text((plot_left + plot_right) / 2, plot_bottom + 44, "Track in the set", "label", "middle"))

    curve_x = scale(0, len(levels) - 1, plot_left, plot_right)
    body.append(polyline([(curve_x(index), to_y(level)) for index, level in enumerate(levels)], "planned"))
    body.append(polyline([(to_x(index + 1), to_y(energy)) for index, energy in enumerate(actual)], "series-line"))
    for index, energy in enumerate(actual):
        position = index + 1
        if position in off_target:
            target = curve_target(levels, index, count)
            body.append(line(to_x(position), to_y(target), to_x(position), to_y(energy), "miss-line"))
            body.append(dot(to_x(position), to_y(energy), "miss-fill"))
        else:
            body.append(dot(to_x(position), to_y(energy), "series-fill", 4))

    legend_x = plot_right + 24
    body.append(line(legend_x, 112, legend_x + 28, 112, "planned"))
    body.append(text(legend_x + 36, 116, "Curve drawn", "label"))
    body.append(line(legend_x, 140, legend_x + 28, 140, "series-line"))
    body.append(dot(legend_x + 14, 140, "series-fill", 4))
    body.append(text(legend_x + 36, 144, "Set built", "label"))
    if off_target:
        body.append(dot(legend_x + 14, 168, "miss-fill"))
        body.append(text(legend_x + 36, 172, "Off the curve", "label"))
    return document(width, height, title, description, body)


def curve_target(levels, index, count):
    if count == 1:
        return levels[0]
    scaled = index / (count - 1) * (len(levels) - 1)
    below = min(int(scaled), len(levels) - 2)
    fraction = scaled - below
    return levels[below] + (levels[below + 1] - levels[below]) * fraction


def planned_and_actual_chart():
    return curve_chart(
        "Planned and actual energy",
        f"{len(ACTUAL_ENERGY)} tracks from a folder of melodic house, curve 4,6,8,9,7,5.",
        CURVE_LEVELS,
        ACTUAL_ENERGY,
        [],
        PLANNED_DESCRIPTION,
    )


def unreachable_chart():
    return curve_chart(
        "A peak the pool cannot reach",
        "The same folder with a curve that asks for 10, which none of its tracks rates.",
        UNREACHABLE_CURVE_LEVELS,
        UNREACHABLE_ACTUAL_ENERGY,
        UNREACHABLE_OFF_TARGET,
        UNREACHABLE_DESCRIPTION,
    )


PLANNED_DESCRIPTION = ""
UNREACHABLE_DESCRIPTION = ""

CHARTS = {
    "planned-and-actual.svg": planned_and_actual_chart,
    "unreachable-peak.svg": unreachable_chart,
}


def main():
    for file_name, build in CHARTS.items():
        (OUTPUT_DIRECTORY / file_name).write_text(build(), encoding="utf-8")


if __name__ == "__main__":
    main()
