# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Writes the key and tempo charts of docs/concepts/accuracy.md as SVG files
next to this script. The beat grid and cue point charts of the same page are
in docs/images/beat-grid.

Run it with any Python 3 from the repository root:

    python3 docs/images/accuracy/make_charts.py

Every number comes from docs/concepts/accuracy.md, which copies it from
bench/results/giantsteps.md. Change the page and the data below together.
"""

from pathlib import Path
from xml.sax.saxutils import escape

OUTPUT_DIRECTORY = Path(__file__).resolve().parent

# "Tuning and key over time", the row "libkeyfinder with tuning correction, as
# Euphoria uses it": the share of the 604 GiantSteps Key previews in each
# outcome. bench/results/giantsteps.md, the key table.
KEY_OUTCOMES = [
    ("Exact", 60.6),
    ("A fifth away", 11.3),
    ("Relative key", 6.8),
    ("Parallel key", 5.5),
    ("Other", 15.9),
]
KEY_OUTCOMES_THAT_MIX_IN_KEY = 3

# "How the tempo is chosen", the table: Accuracy 1 and Accuracy 2 on the 661
# scored GiantSteps Tempo previews. bench/results/giantsteps.md, the tempo
# table.
TEMPO_METHODS = [
    ("Tempogram estimate as it comes", 62.2, 84.7),
    ("Placed between 90 and 180 BPM", 77.6, 85.5),
    ("Full stratum-dsp analysis", 59.9, 84.9),
]
SHIPPED_TEMPO_METHOD = "Placed between 90 and 180 BPM"

# "Key confidence", the table: the share of the held out half at each level
# and the share of it whose key is exact, and "Results": exact on all 604.
KEY_CONFIDENCE = [("High", 40, 74.2), ("Medium", 40, 57.0), ("Low", 20, 41.0)]
KEY_EXACT_ALL_PREVIEWS = 60.6

# "Tempo confidence", the table: the share of the held out half at each level
# and the share of it whose tempo is right modulo octaves.
TEMPO_CONFIDENCE = [("High", 39, 97.7), ("Medium", 42, 89.8), ("Low", 19, 62.5)]

# "What has been checked", the SALAMI row: F-measure of section boundaries
# against the functional annotations on the 227 test tracks, as
# (method, boundaries per track, F within 3 s, F within 0.5 s).
SECTION_BOUNDARIES = [
    ("Euphoria cues", 2.7, 13.2, 3.2),
    ("Euphoria sections", 6.2, 21.2, 4.8),
    ("Foote, librosa features", 16.5, 45.1, 20.7),
    ("A boundary every 20 s", 16.9, 25.6, 4.5),
]

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
.bracket { stroke: #52514e; stroke-width: 1.5; fill: none; }
.connector { stroke: #c3c2b7; stroke-width: 2; fill: none; stroke-linecap: round; }
.series-fill { fill: #2a78d6; }
.series-1-fill { fill: #2a78d6; }
.series-2-fill { fill: #eb6834; }
.ring { stroke: #fcfcfb; stroke-width: 2; }
.halo { stroke: #fcfcfb; stroke-width: 4px; stroke-linejoin: round; paint-order: stroke; }
@media (prefers-color-scheme: dark) {
  .surface { fill: #1a1a19; stroke: rgba(255,255,255,0.10); }
  .title, .value { fill: #f0efec; }
  .subtitle, .label { fill: #c3c2b7; }
  .grid { stroke: #2c2c2a; }
  .axis, .connector { stroke: #383835; }
  .bracket { stroke: #c3c2b7; }
  .series-fill, .series-1-fill { fill: #3987e5; }
  .series-2-fill { fill: #d95926; }
  .ring, .halo { stroke: #1a1a19; }
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


def dot(x, y, css_class, radius=6):
    return (
        f'<circle cx="{number(x)}" cy="{number(y)}" r="{radius}" '
        f'class="{css_class} ring"/>'
    )


def horizontal_bar(left, top, width, height):
    """A bar with a 4px rounded data end and a square baseline end."""
    radius = min(4, width / 2, height / 2)
    right = left + width
    bottom = top + height
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


def key_outcomes_chart():
    width = 800
    row_height = 36
    chart_top = 92
    chart_bottom = chart_top + row_height * len(KEY_OUTCOMES)
    height = chart_bottom + 64
    to_x = scale(0, 70, 150, 640)
    title = "What the detected key is, against the annotated one"
    body = [
        text(24, 36, title, "title"),
        text(24, 58, "604 GiantSteps Key previews. The MIREX score counts exact 1.0, a fifth 0.5, relative 0.3, parallel 0.2.", "subtitle"),
    ]
    for share_tick in (0, 20, 40, 60):
        body.append(line(to_x(share_tick), chart_top, to_x(share_tick), chart_bottom, "grid" if share_tick else "axis"))
        body.append(text(to_x(share_tick), chart_bottom + 20, f"{share_tick}%", "tick", "middle"))
    body.append(text(to_x(30), chart_bottom + 44, "Share of the previews", "label", "middle"))
    for index, (outcome, share) in enumerate(KEY_OUTCOMES):
        row_middle = chart_top + row_height * index + row_height / 2
        body.append(text(to_x(0) - 14, row_middle + 4.5, outcome, "label", "end"))
        body.append(horizontal_bar(to_x(0), row_middle - 10, to_x(share) - to_x(0), 20))
        body.append(text(to_x(share) + 10, row_middle + 4.5, f"{share:.1f}%", "value halo"))

    bracket_x = 660
    bracket_top = chart_top + 8
    bracket_bottom = chart_top + row_height * KEY_OUTCOMES_THAT_MIX_IN_KEY - 8
    body.append(
        polyline(
            [(bracket_x - 8, bracket_top), (bracket_x, bracket_top), (bracket_x, bracket_bottom), (bracket_x - 8, bracket_bottom)],
            "bracket",
        )
    )
    bracket_middle = (bracket_top + bracket_bottom) / 2
    body.append(text(bracket_x + 10, bracket_middle - 4, "Mixes", "label"))
    body.append(text(bracket_x + 10, bracket_middle + 13, "in key", "label"))

    description = (
        "Horizontal bars of how the key Euphoria detects compares with the annotated key on the "
        "604 GiantSteps Key previews: exact for 60.6%, a fifth away for 11.3%, the relative key "
        "for 6.8%, the parallel key for 5.5% and something else for 15.9%. A bracket marks exact, "
        "a fifth away and relative as the outcomes that still mix in key."
    )
    return document(width, height, title, description, body)


def tempo_methods_chart():
    width = 800
    row_height = 44
    chart_top = 120
    chart_bottom = chart_top + row_height * len(TEMPO_METHODS)
    height = chart_bottom + 64
    to_x = scale(50, 100, 290, 740)
    title = "Tempo accuracy, before and after placing it in a range"
    body = [
        text(24, 36, title, "title"),
        text(24, 58, "661 GiantSteps Tempo previews. Euphoria ships the middle row.", "subtitle"),
    ]
    legend_y = 90
    body.append(dot(30, legend_y - 4, "series-1-fill"))
    body.append(text(44, legend_y, "Accuracy 1, within 4%", "label"))
    body.append(dot(230, legend_y - 4, "series-2-fill"))
    body.append(text(244, legend_y, "Accuracy 2, also at 2, 1/2, 3 or 1/3 times the tempo", "label"))

    for share_tick in (50, 60, 70, 80, 90, 100):
        body.append(line(to_x(share_tick), chart_top, to_x(share_tick), chart_bottom, "grid"))
        body.append(text(to_x(share_tick), chart_bottom + 20, f"{share_tick}%", "tick", "middle"))
    body.append(text(to_x(75), chart_bottom + 44, "Share of the previews", "label", "middle"))
    for index, (method, accuracy_1, accuracy_2) in enumerate(TEMPO_METHODS):
        row_middle = chart_top + row_height * index + row_height / 2
        label_class = "value" if method == SHIPPED_TEMPO_METHOD else "label"
        body.append(text(to_x(50) - 14, row_middle + 4.5, method, label_class, "end"))
        body.append(line(to_x(accuracy_1), row_middle, to_x(accuracy_2), row_middle, "connector"))
        body.append(dot(to_x(accuracy_1), row_middle, "series-1-fill"))
        body.append(dot(to_x(accuracy_2), row_middle, "series-2-fill"))
        body.append(text(to_x(accuracy_1) - 12, row_middle + 4.5, f"{accuracy_1:.1f}%", "value halo", "end"))
        body.append(text(to_x(accuracy_2) + 12, row_middle + 4.5, f"{accuracy_2:.1f}%", "value halo"))

    description = (
        "Dot plot of tempo accuracy on the 661 scored GiantSteps Tempo previews. The tempogram "
        "estimate as it comes scores 62.2% in Accuracy 1 and 84.7% in Accuracy 2. Placed between "
        "90 and 180 BPM, which is what Euphoria ships, it scores 77.6% and 85.5%. The full "
        "stratum-dsp analysis scores 59.9% and 84.9%. Placing the tempo in a range lifts "
        "Accuracy 1 by about 15 points and leaves Accuracy 2 almost unchanged."
    )
    return document(width, height, title, description, body)


def section_boundaries_chart():
    width = 800
    row_height = 44
    chart_top = 120
    chart_bottom = chart_top + row_height * len(SECTION_BOUNDARIES)
    height = chart_bottom + 64
    to_x = scale(0, 50, 290, 740)
    title = "Section boundaries against SALAMI, live recordings"
    body = [
        text(24, 36, title, "title"),
        text(24, 58, "227 test tracks, functional annotations. Boundaries per track in brackets.", "subtitle"),
    ]
    legend_y = 90
    body.append(dot(30, legend_y - 4, "series-1-fill"))
    body.append(text(44, legend_y, "F-measure, within 3 s", "label"))
    body.append(dot(230, legend_y - 4, "series-2-fill"))
    body.append(text(244, legend_y, "F-measure, within 0.5 s", "label"))
    for share_tick in (0, 10, 20, 30, 40, 50):
        body.append(line(to_x(share_tick), chart_top, to_x(share_tick), chart_bottom, "grid"))
        body.append(text(to_x(share_tick), chart_bottom + 20, f"{share_tick}%", "tick", "middle"))
    body.append(text(to_x(25), chart_bottom + 44, "F-measure of the boundaries", "label", "middle"))
    for index, (method, per_track, within_3, within_half) in enumerate(SECTION_BOUNDARIES):
        row_middle = chart_top + row_height * index + row_height / 2
        body.append(text(to_x(0) - 14, row_middle + 4.5, f"{method} ({per_track})", "label", "end"))
        body.append(line(to_x(within_half), row_middle, to_x(within_3), row_middle, "connector"))
        body.append(dot(to_x(within_3), row_middle, "series-1-fill"))
        body.append(dot(to_x(within_half), row_middle, "series-2-fill"))
        body.append(text(to_x(within_3) + 12, row_middle + 4.5, f"{within_3:.1f}%", "value halo"))
        body.append(text(to_x(within_half) - 12, row_middle + 4.5, f"{within_half:.1f}%", "value halo", "end"))
    description = (
        "Dot plot of the F-measure of section boundaries on the 227 SALAMI Internet Archive "
        "test tracks, against the functional annotations. Euphoria's cues score 13.2% within "
        "3 seconds and 3.2% within half a second, its section changes 21.2% and 4.8%, a Foote "
        "novelty baseline on librosa features 45.1% and 20.7%, and a boundary every 20 seconds "
        "25.6% and 4.5%."
    )
    return document(width, height, title, description, body)


def confidence_chart(title, subtitle, levels, axis_label, reference, description):
    width = 800
    row_height = 44
    chart_top = 96
    chart_bottom = chart_top + row_height * len(levels)
    height = chart_bottom + 64
    to_x = scale(0, 100, 190, 700)
    body = [
        text(24, 36, title, "title"),
        text(24, 58, subtitle, "subtitle"),
    ]
    for share_tick in (0, 25, 50, 75, 100):
        body.append(line(to_x(share_tick), chart_top, to_x(share_tick), chart_bottom, "grid" if share_tick else "axis"))
        body.append(text(to_x(share_tick), chart_bottom + 20, f"{share_tick}%", "tick", "middle"))
    if reference is not None:
        reference_label, reference_share = reference
        body.append(line(to_x(reference_share), chart_top - 8, to_x(reference_share), chart_bottom, "reference"))
        body.append(text(to_x(reference_share), chart_top - 14, f"{reference_label} {reference_share:.1f}%", "tick halo", "middle"))
    body.append(text(to_x(50), chart_bottom + 44, axis_label, "label", "middle"))
    for index, (level, share_of_half, right) in enumerate(levels):
        row_middle = chart_top + row_height * index + row_height / 2
        body.append(text(to_x(0) - 14, row_middle + 4.5, f"{level}, {share_of_half}% of tracks", "label", "end"))
        body.append(horizontal_bar(to_x(0), row_middle - 10, to_x(right) - to_x(0), 20))
        body.append(text(to_x(right) + 10, row_middle + 4.5, f"{right:.1f}%", "value halo"))
    return document(width, height, title, description, body)


def key_confidence_chart():
    return confidence_chart(
        "How often the key is exact at each confidence",
        "The held out half of the GiantSteps Key previews. A key rated high is right about three times in four.",
        KEY_CONFIDENCE,
        "Share of previews whose key is exact",
        ("All 604 previews", KEY_EXACT_ALL_PREVIEWS),
        "Horizontal bars of how often the detected key is exact on the held out half of the "
        "GiantSteps Key previews, by confidence: 74.2% of the 40% rated high, 57.0% of the 40% "
        "rated medium and 41.0% of the 20% rated low, against 60.6% of all 604 previews.",
    )


def tempo_confidence_chart():
    return confidence_chart(
        "How often the tempo is right at each confidence",
        "The held out half of the GiantSteps Tempo previews, right when within 4% at 1/4 to 4 times the tempo.",
        TEMPO_CONFIDENCE,
        "Share of previews whose tempo is right modulo octaves",
        None,
        "Horizontal bars of how often the tempo is right modulo octaves on the held out half of "
        "the GiantSteps Tempo previews, by confidence: 97.7% of the 39% rated high, 89.8% of the "
        "42% rated medium and 62.5% of the 19% rated low.",
    )


CHARTS = {
    "key-outcomes.svg": key_outcomes_chart,
    "tempo-range.svg": tempo_methods_chart,
    "section-boundaries.svg": section_boundaries_chart,
    "key-confidence.svg": key_confidence_chart,
    "tempo-confidence.svg": tempo_confidence_chart,
}


def main():
    for file_name, build in CHARTS.items():
        (OUTPUT_DIRECTORY / file_name).write_text(build(), encoding="utf-8")


if __name__ == "__main__":
    main()
