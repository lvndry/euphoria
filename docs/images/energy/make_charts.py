"""Writes the charts of docs/concepts/energy.md as SVG files next to this script.

Run it with any Python 3 from the repository root:

    python3 docs/images/energy/make_charts.py

Every number comes from docs/concepts/energy.md. Change the page and the data
below together.
"""

from pathlib import Path
from xml.sax.saxutils import escape

OUTPUT_DIRECTORY = Path(__file__).resolve().parent

# "From measurements to a rating", anchor table: onset rate scores 0 at 2.5
# per second and 1 at 7 per second.
ONSET_RATE_LOW = 2.5
ONSET_RATE_HIGH = 7.0

# "From measurements to a rating", step 3: 0.1 maps to the bottom of the
# scale and 0.9 to the top, cut into ten equal steps.
COMBINED_LOW = 0.1
COMBINED_HIGH = 0.9
RATING_COUNT = 10

# "From measurements to a rating", the Weight column of the anchor table.
FEATURE_WEIGHTS = [
    ("Spectral flux", 0.65),
    ("Integrated loudness", 0.20),
    ("Onset rate", 0.10),
    ("Loudness range", 0.05),
    ("Low frequency share", 0.0),
    ("Tempo, when known", 0.0),
]

# "From measurements to a rating", the row "Tracks, rated with the detected
# tempo" for ratings 1 to 10 of the 604 GiantSteps Key previews.
GIANTSTEPS_TRACKS_PER_RATING = [26, 22, 53, 67, 86, 91, 102, 79, 42, 36]

# "Energy curve": energy_curve on a two minute preview from the GiantSteps
# benchmark set, 8 second windows every 4 seconds, as (window centre in
# seconds, rating), and the rating of the whole preview.
CURVE_WINDOW_SECONDS = 8
CURVE_STEP_SECONDS = 4
CURVE_POINTS = [
    (4, 10), (8, 9), (12, 8), (16, 6), (20, 5), (24, 4), (28, 2), (32, 4),
    (36, 5), (40, 1), (44, 1), (48, 1), (52, 1), (56, 1), (60, 2), (64, 8),
    (68, 9), (72, 9), (76, 10), (80, 9), (84, 9), (88, 10), (92, 9), (96, 10),
    (100, 9), (104, 6), (108, 7), (112, 8), (116, 8),
]
CURVE_WHOLE_TRACK_RATING = 6

# "How well it works", the table of agreement by genre: Spearman correlation
# of the score with listeners' arousal for the old weights (version 1) and the
# new weights (version 2), and the number of songs. DEAM rows are the 586 held
# out songs, PMEmo rows the 226 held out songs with a genre label.
GENRE_AGREEMENT = [
    ("DEAM, all genres", 586, 0.601, 0.656),
    ("Electronic", 110, 0.650, 0.709),
    ("Rock", 135, 0.570, 0.669),
    ("Folk", 99, 0.609, 0.732),
    ("Pop", 81, 0.729, 0.735),
    ("Jazz", 81, 0.688, 0.685),
    ("Classical", 71, 0.454, 0.458),
    ("Country", 71, 0.507, 0.474),
    ("Blues", 62, 0.405, 0.599),
    ("Experimental", 54, 0.694, 0.751),
    ("International", 44, 0.695, 0.787),
    ("Hip-hop", 32, 0.442, 0.621),
    ("Soul and R&B", 36, 0.626, 0.805),
    ("PMEmo, all genres", 226, 0.526, 0.760),
    ("Hip-hop and rap", 32, 0.600, 0.823),
    ("Pop", 26, 0.535, 0.631),
    ("Country", 22, 0.705, 0.775),
    ("Dance and electronic", 19, 0.349, 0.524),
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


def feature_to_score_chart():
    width = 800
    height = 360
    body = [
        text(24, 36, "From a measurement to a rating", "title"),
        text(24, 58, "Each feature becomes a score, and the weighted average of the scores becomes the rating.", "subtitle"),
    ]

    panel_top = 100
    panel_bottom = 300
    body.append(text(64, panel_top - 14, "Onset rate to score", "label"))
    to_x = scale(0, 10, 64, 340)
    to_y = scale(0, 1, panel_bottom, panel_top)
    for score_tick in (0, 0.5, 1):
        body.append(line(to_x(0), to_y(score_tick), to_x(10), to_y(score_tick), "grid" if score_tick else "axis"))
        body.append(text(to_x(0) - 10, to_y(score_tick) + 4, number(score_tick), "tick", "end"))
    for rate_tick in (0, 2.5, 5, 7, 10):
        body.append(text(to_x(rate_tick), panel_bottom + 20, number(rate_tick), "tick", "middle"))
    body.append(text(to_x(5), panel_bottom + 42, "Percussive events per second", "label", "middle"))
    body.append(
        polyline(
            [(to_x(0), to_y(0)), (to_x(ONSET_RATE_LOW), to_y(0)), (to_x(ONSET_RATE_HIGH), to_y(1)), (to_x(10), to_y(1))],
            "series-line",
        )
    )
    body.append(dot(to_x(ONSET_RATE_LOW), to_y(0)))
    body.append(dot(to_x(ONSET_RATE_HIGH), to_y(1)))
    body.append(text(to_x(ONSET_RATE_LOW) + 24, to_y(0) - 10, "2.5 per second scores 0", "label"))
    body.append(text(to_x(ONSET_RATE_HIGH) + 8, to_y(1) + 22, "7 per second scores 1", "label"))

    body.append(text(490, panel_top - 14, "Weighted average to rating", "label"))
    to_x = scale(0, 1, 490, 766)
    to_y = scale(0.5, 10.5, panel_bottom, panel_top)
    for rating_tick in (1, 5, 10):
        body.append(line(to_x(0), to_y(rating_tick), to_x(1), to_y(rating_tick), "grid"))
        body.append(text(to_x(0) - 10, to_y(rating_tick) + 4, str(rating_tick), "tick", "end"))
    body.append(line(to_x(0), panel_bottom, to_x(1), panel_bottom, "axis"))
    for average_tick in (0, COMBINED_LOW, 0.5, COMBINED_HIGH, 1):
        body.append(text(to_x(average_tick), panel_bottom + 20, number(average_tick), "tick", "middle"))
    body.append(text(to_x(0.5), panel_bottom + 42, "Weighted average of the scores", "label", "middle"))
    for anchor in (COMBINED_LOW, COMBINED_HIGH):
        body.append(line(to_x(anchor), panel_top, to_x(anchor), panel_bottom, "reference"))
    step_width = (COMBINED_HIGH - COMBINED_LOW) / RATING_COUNT
    staircase = [(to_x(0), to_y(1))]
    for rating in range(1, RATING_COUNT):
        edge = COMBINED_LOW + step_width * rating
        staircase.append((to_x(edge), to_y(rating)))
        staircase.append((to_x(edge), to_y(rating + 1)))
    staircase.append((to_x(1), to_y(RATING_COUNT)))
    body.append(polyline(staircase, "series-line"))
    body.append(text(to_x(COMBINED_LOW) + 8, to_y(9.6), "0.1 is the bottom", "label"))
    body.append(text(to_x(COMBINED_HIGH) - 8, to_y(1.4), "0.9 is the top", "label", "end"))

    description = (
        "Left: onset rate scores 0 at or below 2.5 percussive events per second, "
        "rises in a straight line, and scores 1 at or above 7 per second. "
        "Right: the weighted average of the feature scores maps to the rating as a "
        "staircase. An average of 0.1 or less gives 1, 0.9 or more gives 10, and "
        "the stretch between is cut into ten equal steps."
    )
    return document(width, height, "From a measurement to a rating", description, body)


def weights_chart():
    ranked = sorted(FEATURE_WEIGHTS, key=lambda feature: -feature[1])
    row_height = 36
    chart_top = 92
    width = 800
    height = chart_top + row_height * len(ranked) + 20
    to_x = scale(0, 0.7, 220, 700)
    body = [
        text(24, 36, "How much each feature counts", "title"),
        text(24, 58, "Weights of the six features in the average. Spectral flux counts most, two features count nothing.", "subtitle"),
    ]
    for index, (name, weight) in enumerate(ranked):
        row_middle = chart_top + row_height * index + row_height / 2
        body.append(text(to_x(0) - 14, row_middle + 4.5, name, "label", "end"))
        body.append(bar_path(to_x(0), row_middle - 10, to_x(weight) - to_x(0), 20, "right"))
        body.append(text(to_x(weight) + 10, row_middle + 4.5, f"{weight:.2f}", "value"))
    body.append(line(to_x(0), chart_top, to_x(0), chart_top + row_height * len(ranked), "axis"))

    description = (
        "Horizontal bars of the weights: spectral flux 0.65, integrated loudness 0.20, "
        "onset rate 0.10, loudness range 0.05, and low frequency share and tempo 0 each."
    )
    return document(width, height, "How much each feature counts", description, body)


def giantsteps_spread_chart():
    width = 800
    height = 360
    plot_top = 96
    plot_bottom = 290
    to_x = scale(0.5, 10.5, 80, 770)
    to_y = scale(0, 120, plot_bottom, plot_top)
    body = [
        text(24, 36, "How the GiantSteps previews spread over the scale", "title"),
        text(24, 58, "Number of the 604 previews at each rating, rated with the detected tempo.", "subtitle"),
    ]
    for count_tick in (0, 40, 80, 120):
        body.append(line(to_x(0.5), to_y(count_tick), to_x(10.5), to_y(count_tick), "grid" if count_tick else "axis"))
        body.append(text(to_x(0.5) - 10, to_y(count_tick) + 4, str(count_tick), "tick", "end"))
    bar_width = 24
    peak_count = max(GIANTSTEPS_TRACKS_PER_RATING)
    for rating, count in enumerate(GIANTSTEPS_TRACKS_PER_RATING, start=1):
        center = to_x(rating)
        body.append(bar_path(center - bar_width / 2, to_y(count), bar_width, plot_bottom - to_y(count), "top"))
        body.append(text(center, plot_bottom + 20, str(rating), "tick", "middle"))
        if count == peak_count:
            body.append(text(center, to_y(count) - 10, f"{count} tracks", "value", "middle"))
    body.append(text(to_x(5.5), plot_bottom + 44, "Energy rating", "label", "middle"))

    description = (
        "Column chart of how many of the 604 GiantSteps Key previews get each rating "
        "with the detected tempo: 26, 22, 53, 67, 86, 91, 102, 79, 42 and 36 tracks "
        "for ratings 1 to 10. The previews cover the whole scale, with most between 4 and 8 "
        "and the peak at 7."
    )
    return document(width, height, "How the GiantSteps previews spread over the scale", description, body)


def minutes_and_seconds(seconds):
    return f"{seconds // 60}:{seconds % 60:02d}"


def energy_curve_chart():
    width = 800
    height = 360
    plot_top = 100
    plot_bottom = 300
    plot_right = 680
    to_x = scale(0, 120, 64, plot_right)
    to_y = scale(1, 10, plot_bottom, plot_top)
    body = [
        text(24, 36, "Energy curve of a two minute preview", "title"),
        text(24, 58, "A rating every 4 seconds, each over an 8 second window, against the rating of the whole preview.", "subtitle"),
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
    body.append(line(to_x(0), to_y(CURVE_WHOLE_TRACK_RATING), to_x(120), to_y(CURVE_WHOLE_TRACK_RATING), "reference"))
    body.append(polyline(steps, "series-line"))
    body.append(text(plot_right + 10, to_y(CURVE_WHOLE_TRACK_RATING) - 4, "Whole preview", "label"))
    body.append(text(plot_right + 10, to_y(CURVE_WHOLE_TRACK_RATING) + 13, f"rates {CURVE_WHOLE_TRACK_RATING}", "label"))
    body.append(text(to_x(49), to_y(1) - 14, "Breakdown", "label", "middle"))
    body.append(text(to_x(62) - 10, to_y(9.2), "Drop", "label", "end"))

    description = (
        "Step chart of the energy rating of a two minute preview from the benchmark set, "
        "one rating every 4 seconds over 8 second windows. It starts at 10, eases down to 2 "
        "by half a minute, falls to 1 in a breakdown between about 40 and 60 seconds, jumps "
        "back to 8 and then 9 or 10 at the drop just after one minute and stays high to the "
        "end. The whole preview rates 6, "
        "so the windows reach both higher and lower than the rating of the whole track."
    )
    return document(width, height, "Energy curve of a two minute preview", description, body)


def agreement_by_genre_chart():
    width = 800
    row_height = 26
    chart_top = 104
    height = chart_top + row_height * len(GENRE_AGREEMENT) + 64
    to_x = scale(0.3, 0.9, 230, 760)
    chart_bottom = chart_top + row_height * len(GENRE_AGREEMENT)
    body = [
        text(24, 36, "Agreement with listeners by genre", "title"),
        text(24, 58, "Correlation of the energy score with listeners' arousal, on songs held out from the fitting.", "subtitle"),
        text(24, 80, "Grey: weights of version 1. Blue: weights of version 2.", "subtitle"),
    ]
    for tick in (0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9):
        body.append(line(to_x(tick), chart_top, to_x(tick), chart_bottom, "grid"))
        body.append(text(to_x(tick), chart_bottom + 20, f"{tick:.1f}", "tick", "middle"))
    body.append(text(to_x(0.6), chart_bottom + 44, "Spearman correlation", "label", "middle"))
    for index, (name, count, before, after) in enumerate(GENRE_AGREEMENT):
        row_middle = chart_top + row_height * index + row_height / 2
        body.append(text(to_x(0.3) - 14, row_middle + 4.5, f"{name} ({count})", "label", "end"))
        body.append(line(to_x(before), row_middle, to_x(after), row_middle, "reference"))
        body.append(f'<circle cx="{number(to_x(before))}" cy="{number(row_middle)}" r="5" class="muted-fill ring"/>')
        body.append(dot(to_x(after), row_middle))

    description = (
        "Dot plot of the correlation between the energy score and listeners' arousal, by genre, "
        "for the weights of version 1 and of version 2. Version 2 is higher in 13 of 16 genre "
        "groups, level in jazz and classical, and 0.03 lower in DEAM country music. Overall it "
        "rises from 0.60 to 0.66 on DEAM and from 0.53 to 0.76 on PMEmo."
    )
    return document(width, height, "Agreement with listeners by genre", description, body)


CHARTS = {
    "feature-to-rating.svg": feature_to_score_chart,
    "weights.svg": weights_chart,
    "giantsteps-spread.svg": giantsteps_spread_chart,
    "agreement-by-genre.svg": agreement_by_genre_chart,
    "energy-curve.svg": energy_curve_chart,
}


def main():
    for file_name, build in CHARTS.items():
        (OUTPUT_DIRECTORY / file_name).write_text(build(), encoding="utf-8")


if __name__ == "__main__":
    main()
