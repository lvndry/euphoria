"""Writes the charts of docs/concepts/audio-quality.md as SVG files next to this script.

Run it with any Python 3 from the repository root:

    python3 docs/images/audio-quality/make_charts.py

Every number comes from bench/results/quality.md, the table "Encodings of
music", and from the thresholds in docs/concepts/audio-quality.md. Change them
together.
"""

from pathlib import Path
from xml.sax.saxutils import escape

OUTPUT_DIRECTORY = Path(__file__).resolve().parent

# Where the spectrum ends when nothing removed it: half of 44.1 kHz.
FULL_BAND_KHZ = 22.05

# "Where the thresholds come from": the average bit rates that start
# Acceptable and Good.
ACCEPTABLE_FROM_KBPS = 144
GOOD_FROM_KBPS = 200

# "Where the thresholds come from": below these cutoffs, in kHz, a file of
# that average bit rate is likely re-encoded, as (lowest bit rate, cutoff).
# A lossless file is likely re-encoded below 19 kHz.
EXPECTED_CUTOFF_STEPS = [(0, 14.5), (144, 15.5), (200, 18.0), (288, 19.5)]
LOSSLESS_EXPECTED_CUTOFF_KHZ = 19.0

# bench/results/quality.md, "Encodings of music": seven files per row, as
# (encoding, median measured bit rate in kbit/s, lowest cutoff in kHz,
# highest cutoff in kHz, files with no cutoff). None means no file of the
# row had a cutoff.
ENCODED_ONCE = [
    ("MP3 96", 96, 15.2, 15.4, 0),
    ("MP3 128", 128, 16.0, 16.8, 0),
    ("MP3 192", 192, 18.6, 18.9, 1),
    ("MP3 256", 256, 19.3, 19.6, 0),
    ("MP3 320", 320, 20.1, 20.3, 0),
    ("FFmpeg AAC 96", 97, 15.9, 16.7, 0),
    ("FFmpeg AAC 128", 129, 17.3, 17.3, 0),
    ("FFmpeg AAC 192", 194, 19.3, 19.4, 0),
    ("FFmpeg AAC 256", 257, None, None, 7),
    ("FFmpeg AAC 320", 283, None, None, 7),
    ("Apple AAC 96", 92, 15.6, 15.8, 1),
    ("Apple AAC 128", 120, 17.3, 18.5, 3),
    ("Apple AAC 192", 176, 19.4, 19.7, 3),
    ("Apple AAC 256", 245, None, None, 7),
    ("Apple AAC 320", 317, None, None, 7),
]
# The same table, the transcodes: a lossy file decoded and encoded again at a
# higher bit rate. The bit rate of the FLAC transcode is lossless.
TRANSCODED = [
    ("MP3 128 to MP3 320", 320, 16.0, 16.8, 0),
    ("MP3 128 to FLAC", None, 16.0, 16.8, 0),
    ("MP3 192 to MP3 320", 320, 18.6, 18.9, 1),
    ("FFmpeg AAC 128 to 256", 223, 17.2, 17.3, 0),
    ("MP3 128 to Apple AAC 256", 245, 16.0, 16.8, 0),
    ("Apple AAC 128 to 320", 309, 17.3, 18.5, 3),
]

# The same table, the column "Error below 15 kHz (dB)", for 96, 128, 192, 256
# and 320 kbit/s.
NOMINAL_BITRATES = [96, 128, 192, 256, 320]
ENCODING_ERROR_DB = [
    ("MP3 (LAME)", [-24.1, -25.6, -30.3, -46.7, -52.1]),
    ("FFmpeg AAC", [-25.5, -31.0, -36.0, -42.4, -44.1]),
    ("Apple AAC", [-29.4, -32.6, -39.7, -42.2, -43.4]),
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
.reference { stroke: #52514e; stroke-width: 2; fill: none; stroke-linecap: round; }
.zone { fill: #898781; fill-opacity: 0.12; }
.series-1-fill { fill: #2a78d6; }
.series-2-fill { fill: #eb6834; }
.series-3-fill { fill: #1baf7a; }
.series-1-line { stroke: #2a78d6; }
.series-2-line { stroke: #eb6834; }
.series-3-line { stroke: #1baf7a; }
.line { stroke-width: 2; fill: none; stroke-linejoin: round; stroke-linecap: round; }
.ring { stroke: #fcfcfb; stroke-width: 2; }
@media (prefers-color-scheme: dark) {
  .surface { fill: #1a1a19; stroke: rgba(255,255,255,0.10); }
  .title, .value { fill: #f0efec; }
  .subtitle, .label { fill: #c3c2b7; }
  .grid { stroke: #2c2c2a; }
  .axis { stroke: #383835; }
  .reference { stroke: #c3c2b7; }
  .zone { fill-opacity: 0.18; }
  .series-1-fill { fill: #3987e5; }
  .series-2-fill { fill: #d95926; }
  .series-3-fill { fill: #199e70; }
  .series-1-line { stroke: #3987e5; }
  .series-2-line { stroke: #d95926; }
  .series-3-line { stroke: #199e70; }
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


def range_bar(left, right, middle, height, css_class):
    """A horizontal bar from `left` to `right`, both ends rounded by 4px, at
    least 8px long so a single value still shows."""
    if right - left < 8:
        centre = (left + right) / 2
        left, right = centre - 4, centre + 4
    top = middle - height / 2
    return (
        f'<rect x="{number(left)}" y="{number(top)}" width="{number(right - left)}" '
        f'height="{number(height)}" rx="4" class="{css_class}"/>'
    )


def swatch(x, y, css_class):
    return f'<rect x="{number(x)}" y="{number(y - 9)}" width="14" height="10" rx="3" class="{css_class}"/>'


def scale(domain_low, domain_high, range_low, range_high):
    def mapped(value):
        fraction = (value - domain_low) / (domain_high - domain_low)
        return range_low + fraction * (range_high - range_low)

    return mapped


def expected_cutoff_khz(bitrate_kbps):
    if bitrate_kbps is None:
        return LOSSLESS_EXPECTED_CUTOFF_KHZ
    cutoff = EXPECTED_CUTOFF_STEPS[0][1]
    for lowest_bitrate, step_cutoff in EXPECTED_CUTOFF_STEPS:
        if bitrate_kbps >= lowest_bitrate:
            cutoff = step_cutoff
    return cutoff


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


def full_band_note(full_band_count, has_cutoff):
    if full_band_count == 0:
        return ""
    if not has_cutoff:
        return "no cutoff in 7 of 7"
    return f"no cutoff in {full_band_count} of 7"


def cutoffs_chart():
    width = 800
    row_height = 24
    section_gap = 40
    plot_left = 220
    plot_right = 640
    chart_top = 128
    to_x = scale(14, 22, plot_left, plot_right)
    rows_top = chart_top + 24
    transcoded_top = rows_top + row_height * len(ENCODED_ONCE) + section_gap
    plot_bottom = transcoded_top + row_height * len(TRANSCODED)
    height = plot_bottom + 60

    body = [
        text(24, 36, "Where the content stops", "title"),
        text(24, 58, "Cutoff of seven tracks per encoding, lowest to highest. A transcode keeps the cutoff of the file it was made from.", "subtitle"),
        swatch(24, 90, "series-1-fill"),
        text(44, 90, "Encoded once from lossless", "label"),
        swatch(236, 90, "series-2-fill"),
        text(256, 90, "Encoded again at a higher bit rate", "label"),
        line(490, 80, 490, 92, "reference"),
        text(502, 90, "Likely re-encoded left of the tick", "label"),
    ]
    for kilohertz in range(14, 23, 2):
        body.append(line(to_x(kilohertz), chart_top + 8, to_x(kilohertz), plot_bottom, "grid"))
        body.append(text(to_x(kilohertz), plot_bottom + 20, f"{kilohertz} kHz", "tick", "middle"))
    body.append(text((plot_left + plot_right) / 2, plot_bottom + 44, "Frequency where the content stops", "label", "middle"))

    def add_rows(rows, top, css_class):
        for index, (encoding, bitrate, lowest, highest, full_band_count) in enumerate(rows):
            middle = top + row_height * index + row_height / 2
            body.append(text(plot_left - 14, middle + 4.5, encoding, "label", "end"))
            if lowest is not None:
                body.append(range_bar(to_x(lowest), to_x(highest), middle, 12, css_class))
            if bitrate is None or bitrate >= ACCEPTABLE_FROM_KBPS:
                expected = to_x(expected_cutoff_khz(bitrate))
                body.append(line(expected, middle - 9, expected, middle + 9, "reference"))
            note = full_band_note(full_band_count, lowest is not None)
            if note:
                body.append(text(plot_right + 12, middle + 4.5, note, "tick"))

    body.append(text(24, chart_top + 12, "From a lossless source", "value"))
    add_rows(ENCODED_ONCE, rows_top, "series-1-fill")
    body.append(text(24, transcoded_top - 12, "Transcoded from a low bit rate", "value"))
    add_rows(TRANSCODED, transcoded_top, "series-2-fill")

    description = (
        "Range bars of the frequency where the content stops, for seven tracks per "
        "encoding. From 144 kbit/s, and for lossless files, a tick marks the cutoff "
        "below which the file is likely re-encoded. Below 144 kbit/s the verdict is "
        "weak regardless of cutoff. Each encoding made once from a lossless source "
        "with a tick sits at or right of it: MP3 cuts at about 15.3 kHz at 96 kbit/s, 16 to 16.8 kHz at "
        "128, 18.6 to 18.9 at 192, 19.3 to 19.6 at 256 and 20.1 to 20.3 at 320; the AAC "
        "encoders cut higher or not at all. Transcoded to a higher bit rate or to FLAC, "
        "the bars stay where the low bit rate source put them, at 16 to 18.9 kHz, left "
        "of their tick."
    )
    return document(width, height, "Where the content stops", description, body)


def verdicts_chart():
    width = 800
    height = 500
    plot_left = 80
    plot_right = 700
    plot_top = 140
    plot_bottom = 430
    to_x = scale(80, 340, plot_left, plot_right)
    to_y = scale(14, FULL_BAND_KHZ, plot_bottom, plot_top)

    body = [
        text(24, 36, "How bit rate and cutoff make the verdict", "title"),
        text(24, 58, "Bit rate sets Weak, Acceptable or Good. From 144 kbit/s, a cutoff below the line marks a likely re-encode.", "subtitle"),
        '<circle cx="30" cy="85" r="5" class="series-1-fill ring"/>',
        text(42, 90, "Encoded once from lossless", "label"),
        '<circle cx="236" cy="85" r="5" class="series-2-fill ring"/>',
        text(248, 90, "Encoded again at a higher bit rate", "label"),
    ]

    zone = [(to_x(ACCEPTABLE_FROM_KBPS), plot_bottom)]
    for index, (lowest_bitrate, cutoff) in enumerate(EXPECTED_CUTOFF_STEPS):
        if lowest_bitrate < ACCEPTABLE_FROM_KBPS:
            continue
        start = lowest_bitrate
        end = EXPECTED_CUTOFF_STEPS[index + 1][0] if index + 1 < len(EXPECTED_CUTOFF_STEPS) else 340
        zone.append((to_x(start), to_y(cutoff)))
        zone.append((to_x(end), to_y(cutoff)))
    zone.append((to_x(340), plot_bottom))
    zone_points = " ".join(f"{number(x)},{number(y)}" for x, y in zone)
    body.append(f'<polygon points="{zone_points}" class="zone"/>')

    for kilohertz in (14, 16, 18, 20, 22):
        body.append(line(plot_left, to_y(kilohertz), plot_right, to_y(kilohertz), "grid" if kilohertz != 14 else "axis"))
        body.append(text(plot_left - 10, to_y(kilohertz) + 4, f"{kilohertz} kHz", "tick", "end"))
    for bitrate in (96, 128, 160, 192, 224, 256, 288, 320):
        body.append(text(to_x(bitrate), plot_bottom + 20, str(bitrate), "tick", "middle"))
    body.append(text((plot_left + plot_right) / 2, plot_bottom + 44, "Average bit rate measured, kbit/s", "label", "middle"))

    for boundary in (ACCEPTABLE_FROM_KBPS, GOOD_FROM_KBPS):
        body.append(line(to_x(boundary), plot_top - 8, to_x(boundary), plot_bottom, "axis"))
    band_label_y = plot_top - 14
    body.append(text((to_x(80) + to_x(ACCEPTABLE_FROM_KBPS)) / 2, band_label_y, "Weak on a big system", "value", "middle"))
    body.append(text((to_x(ACCEPTABLE_FROM_KBPS) + to_x(GOOD_FROM_KBPS)) / 2, band_label_y, "Acceptable", "value", "middle"))
    body.append(text((to_x(GOOD_FROM_KBPS) + to_x(340)) / 2, band_label_y, "Good", "value", "middle"))

    step_points = []
    for index, (lowest_bitrate, cutoff) in enumerate(EXPECTED_CUTOFF_STEPS):
        if lowest_bitrate < ACCEPTABLE_FROM_KBPS:
            continue
        start = lowest_bitrate
        end = EXPECTED_CUTOFF_STEPS[index + 1][0] if index + 1 < len(EXPECTED_CUTOFF_STEPS) else 340
        step_points.append((to_x(start), to_y(cutoff)))
        step_points.append((to_x(end), to_y(cutoff)))
    body.append(polyline(step_points, "reference"))
    body.append(text(to_x(256), to_y(14.6), "Likely re-encoded", "value", "middle"))

    for _, bitrate, lowest, _, _ in ENCODED_ONCE:
        cutoff = FULL_BAND_KHZ if lowest is None else lowest
        body.append(dot(to_x(bitrate), to_y(cutoff), "series-1-fill"))
    for _, bitrate, lowest, _, _ in TRANSCODED:
        if bitrate is None:
            continue
        body.append(dot(to_x(bitrate), to_y(lowest), "series-2-fill"))
    body.append(text(to_x(320) - 12, to_y(16.0) + 4.5, "MP3 128 made 320", "label", "end"))
    body.append(text(to_x(320) + 12, to_y(20.1) + 4.5, "MP3 320", "label"))
    body.append(text(to_x(128) + 12, to_y(16.0) + 4.5, "MP3 128", "label"))
    body.append(text(plot_right + 12, to_y(FULL_BAND_KHZ) + 4, "full band", "tick"))

    description = (
        "Scatter of the lowest cutoff of each encoding against its measured average "
        "bit rate, over the verdict zones. Below 144 kbit/s is weak on a big system, "
        "144 to 199 acceptable, 200 and above good. Below 144 kbit/s the verdict stays "
        "weak regardless of cutoff. From 144 kbit/s, a step line marks the cutoff below "
        "which the file is likely re-encoded: 15.5 kHz to 199 kbit/s, 18 kHz to 287 and "
        "19.5 kHz from 288. The lowest cutoff of each encoding made once from a "
        "lossless source sits above the line, MP3 at 256 kbit/s closest at 19.3 kHz, 1.3 kHz above it. "
        "The lowest cutoff of each transcode to a higher bit rate sits below it. "
        "Some files have no cutoff and cannot be identified as re-encoded. Lossless files, not shown, "
        "are likely re-encoded below 19 kHz."
    )
    return document(width, height, "How bit rate and cutoff make the verdict", description, body)


def encoding_error_chart():
    width = 800
    height = 380
    plot_left = 80
    plot_right = 640
    plot_top = 110
    plot_bottom = 310
    to_x = scale(80, 336, plot_left, plot_right)
    to_y = scale(-55, -20, plot_bottom, plot_top)
    series_classes = ["series-1", "series-2", "series-3"]

    body = [
        text(24, 36, "How much the encoder changes below 15 kHz", "title"),
        text(24, 58, "Median difference from the source, relative to the music. Lower is closer to the original.", "subtitle"),
    ]
    legend_x = 24
    for (encoder, _), css_class in zip(ENCODING_ERROR_DB, series_classes):
        body.append(f'<circle cx="{legend_x + 6}" cy="85" r="5" class="{css_class}-fill ring"/>')
        body.append(text(legend_x + 18, 90, encoder, "label"))
        legend_x += 140
    for decibels in (-20, -30, -40, -50):
        body.append(line(plot_left, to_y(decibels), plot_right, to_y(decibels), "grid"))
        body.append(text(plot_left - 10, to_y(decibels) + 4, f"{decibels} dB", "tick", "end"))
    body.append(line(plot_left, plot_bottom, plot_right, plot_bottom, "axis"))
    for bitrate in NOMINAL_BITRATES:
        body.append(text(to_x(bitrate), plot_bottom + 20, str(bitrate), "tick", "middle"))
    body.append(text((plot_left + plot_right) / 2, plot_bottom + 44, "Bit rate the encoder was set to, kbit/s", "label", "middle"))

    for (encoder, errors), css_class in zip(ENCODING_ERROR_DB, series_classes):
        points = [(to_x(bitrate), to_y(error)) for bitrate, error in zip(NOMINAL_BITRATES, errors)]
        body.append(polyline(points, f"{css_class}-line line"))
        for x, y in points:
            body.append(dot(x, y, f"{css_class}-fill", radius=4))
        last_x, last_y = points[-1]
        offset = {"MP3 (LAME)": 4.5, "FFmpeg AAC": 14, "Apple AAC": -4}[encoder]
        body.append(text(last_x + 12, last_y + offset, f"{encoder} {errors[-1]:.0f} dB", "label"))
    body.append(text(to_x(236) - 10, to_y(-49), "MP3's big step, 192 to 256", "label", "end"))

    description = (
        "Line chart of the median encoding error below 15 kHz against the bit rate the "
        "encoder was set to, for three encoders. MP3 goes from -24 dB at 96 kbit/s to "
        "-30 dB at 192, then drops to -47 dB at 256 and -52 dB at 320: its big step is "
        "from 192 to 256. FFmpeg AAC goes from -26 to -44 dB and Apple AAC from -29 to "
        "-43 dB, with their biggest steps between 128 and 192 kbit/s."
    )
    return document(width, height, "How much the encoder changes below 15 kHz", description, body)


CHARTS = {
    "cutoffs.svg": cutoffs_chart,
    "verdicts.svg": verdicts_chart,
    "encoding-error.svg": encoding_error_chart,
}


def main():
    for file_name, build in CHARTS.items():
        (OUTPUT_DIRECTORY / file_name).write_text(build(), encoding="utf-8")


if __name__ == "__main__":
    main()
