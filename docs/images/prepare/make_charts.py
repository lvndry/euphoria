"""Writes the chart of docs/guides/prepare-files.md as an SVG file next to this script.

Run it with any Python 3 from the repository root:

    python3 docs/images/prepare/make_charts.py

The curve is the rule of crates/euphoria-core/src/prepare/mod.rs, evaluated
for one example track. Change the guide and the numbers below together.
"""

from pathlib import Path
from xml.sax.saxutils import escape

OUTPUT_DIRECTORY = Path(__file__).resolve().parent

# The defaults of `euphoria prepare`.
TARGET_LUFS = -9.0
TRUE_PEAK_CEILING_DB = -1.0
MAXIMUM_LIMITING_DB = 3.0

# The example track: measured at -15 LUFS, so it wants 6 dB more gain.
EXAMPLE_LUFS = -15.0
WANTED_GAIN_DB = TARGET_LUFS - EXAMPLE_LUFS

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
.muted-fill { fill: #898781; }
.ring { stroke: #fcfcfb; stroke-width: 2; }
@media (prefers-color-scheme: dark) {
  .surface { fill: #1a1a19; stroke: rgba(255,255,255,0.10); }
  .title, .value { fill: #f0efec; }
  .subtitle, .label { fill: #c3c2b7; }
  .grid { stroke: #2c2c2a; }
  .axis { stroke: #383835; }
  .series-line { stroke: #3987e5; }
  .series-fill, .series-wash { fill: #3987e5; }
  .muted-fill { fill: #898781; }
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



def gain_for(true_peak_db):
    allowed = TRUE_PEAK_CEILING_DB - true_peak_db + MAXIMUM_LIMITING_DB
    return min(WANTED_GAIN_DB, allowed)


def limiting_for(true_peak_db):
    return max(0.0, true_peak_db + gain_for(true_peak_db) - TRUE_PEAK_CEILING_DB)


def gain_chart():
    width = 800
    height = 380
    plot_top = 104
    plot_bottom = 300
    to_x = scale(-14, 0, 80, 760)
    to_y = scale(0, 8, plot_bottom, plot_top)
    body = [
        text(24, 36, "How much gain a track gets", "title"),
        text(24, 58, "A track at -15 LUFS wants +6 dB to reach -9 LUFS. Its true peak decides how much of that it gets.", "subtitle"),
    ]
    for gain_tick in (0, 2, 4, 6, 8):
        body.append(line(to_x(-14), to_y(gain_tick), to_x(0), to_y(gain_tick), "grid" if gain_tick else "axis"))
        body.append(text(to_x(-14) - 10, to_y(gain_tick) + 4, f"+{gain_tick}", "tick", "end"))
    for peak_tick in range(-14, 1, 2):
        body.append(text(to_x(peak_tick), plot_bottom + 20, str(peak_tick), "tick", "middle"))
    body.append(text(to_x(-7), plot_bottom + 44, "True peak of the original, in dBTP", "label", "middle"))
    body.append(text(24, plot_top - 26, "Gain applied, in dB", "label"))

    cap_start = TRUE_PEAK_CEILING_DB + MAXIMUM_LIMITING_DB - WANTED_GAIN_DB
    body.append(line(to_x(cap_start), plot_top, to_x(cap_start), plot_bottom, "reference"))
    peaks = [-14 + step * 0.25 for step in range(57)]
    body.append(polyline([(to_x(peak), to_y(gain_for(peak))) for peak in peaks], "series-line"))
    body.append(dot(to_x(cap_start), to_y(WANTED_GAIN_DB)))
    body.append(text(to_x(-13.6), to_y(WANTED_GAIN_DB) - 12, "Target reached: +6 dB", "label"))
    body.append(text(to_x(cap_start) + 12, to_y(WANTED_GAIN_DB) - 12, "Capped from here on", "label"))
    body.append(text(to_x(0) - 8, to_y(gain_for(0)) + 28, "A peak at 0 dBTP gets +2 dB", "label", "end"))

    description = (
        "Line chart of the gain applied against the true peak of the original for a track "
        "that wants 6 dB. Up to a true peak of -4 dBTP the track gets the full 6 dB. "
        "Above that the gain falls one dB for every dB of peak, because the limiter may "
        "turn peaks down by 3 dB at most under a ceiling of -1 dBTP, down to 2 dB at 0 dBTP."
    )
    return document(width, height, "How much gain a track gets", description, body)


def main():
    (OUTPUT_DIRECTORY / "gain-by-true-peak.svg").write_text(gain_chart())


if __name__ == "__main__":
    main()
