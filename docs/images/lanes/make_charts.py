"""Writes the figures of the lanes section in docs/guides/desktop-app.md as SVG
files next to this script, in the style of docs/images/energy/make_charts.py.

Run it with any Python 3 from the repository root:

    python3 docs/images/lanes/make_charts.py

The energy ratings are those of docs/images/energy/make_charts.py. The window
levels are the vocals stem of one separated track, measured with
euphoria_core::stems::VocalLevels, one level in dB below full scale per
window; the threshold is the one in crates/euphoria-core/src/stems/vocal_activity.rs.
"""

import sys
from pathlib import Path

OUTPUT_DIRECTORY = Path(__file__).resolve().parent
sys.path.insert(0, str(OUTPUT_DIRECTORY.parent / "energy"))

import make_charts as energy

LANES_STYLE = """
.lane-back { fill: #e1e0d9; fill-opacity: 0.55; }
.wave { fill: #898781; fill-opacity: 0.55; }
.energy-bar { fill: #2a78d6; fill-opacity: 0.7; }
.vocal-fill { fill: #2a78d6; fill-opacity: 0.7; }
.lane-name { fill: #52514e; font-size: 11px; }
.bubble { fill: #0b0b0b; }
.bubble-text { fill: #fcfcfb; font-size: 12px; }
.pointer { stroke: #0b0b0b; stroke-width: 1; stroke-dasharray: 3 3; fill: none; }
.threshold { stroke: #c4472f; stroke-width: 1.5; fill: none; }
.threshold-label { fill: #c4472f; font-size: 13px; }
.level-fill { fill: #898781; }
.level-voice { fill: #2a78d6; }
@media (prefers-color-scheme: dark) {
  .lane-back { fill: #383835; fill-opacity: 0.8; }
  .wave { fill: #c3c2b7; fill-opacity: 0.5; }
  .energy-bar, .vocal-fill, .level-voice { fill: #3987e5; }
  .lane-name { fill: #c3c2b7; }
  .bubble { fill: #f0efec; }
  .bubble-text { fill: #1a1a19; }
  .pointer { stroke: #f0efec; }
  .threshold, .threshold-label { stroke: #e8765f; }
  .threshold-label { fill: #e8765f; stroke: none; }
  .level-fill { fill: #898781; }
}
"""
energy.STYLE += LANES_STYLE

VOCAL_STRETCHES_SECONDS = [(8, 38), (64, 104)]

WINDOW_LEVELS_DB = [
    -73, -74, -75, -43, -31, -24, -22, -20, -19, -18, -18, -22, -46, -69, -75,
    -73, -73, -77, -79, -78, -31, -22, -19, -18, -17, -18, -18, -17, -16, -16,
    -17, -16, -16, -20, -40, -50, -74, -74, -74, -74, -75, -77, -79,
]
LEVEL_REFERENCE_SHARE = 0.9
LEVEL_RELATIVE_DB = 20
LEVEL_FLOOR_DB = -50


def bubble(x, y, content, pointer_to_y):
    width = 8 * len(content) + 20
    left = x - width / 2
    return [
        energy.line(x, y + 24, x, pointer_to_y, "pointer"),
        f'<rect x="{energy.number(left)}" y="{energy.number(y)}" width="{energy.number(width)}" '
        'height="24" rx="6" class="bubble"/>',
        energy.text(x, y + 16, content, "bubble-text", "middle"),
    ]


def lanes_chart():
    width = 800
    height = 330
    left = 64
    right = 736
    to_x = energy.scale(0, 120, left, right)
    body = [
        energy.text(24, 36, "The lanes under the waveform", "title"),
        energy.text(24, 58, "Each lane is as wide as the waveform, so what is under a part of it belongs to it.", "subtitle"),
    ]

    wave_top, wave_bottom = 150, 210
    middle = (wave_top + wave_bottom) / 2
    for centre, rating in energy.CURVE_POINTS:
        half_height = 6 + rating * 2.4
        body.append(
            f'<rect x="{energy.number(to_x(centre - 2))}" y="{energy.number(middle - half_height / 2)}" '
            f'width="{energy.number(to_x(centre + 2) - to_x(centre - 2) - 1)}" '
            f'height="{energy.number(half_height)}" class="wave"/>'
        )
    body.append(energy.text(left, wave_top - 8, "Waveform", "lane-name"))

    energy_top, energy_bottom = 218, 248
    body.append(f'<rect x="{left}" y="{energy_top}" width="{right - left}" height="{energy_bottom - energy_top}" rx="2" class="lane-back"/>')
    for centre, rating in energy.CURVE_POINTS:
        bar_height = (energy_bottom - energy_top) * rating / 10
        body.append(
            f'<rect x="{energy.number(to_x(centre - 2))}" y="{energy.number(energy_bottom - bar_height)}" '
            f'width="{energy.number(to_x(centre + 2) - to_x(centre - 2))}" '
            f'height="{energy.number(bar_height)}" class="energy-bar"/>'
        )
    body.append(energy.text(left + 4, energy_top + 11, "Energy", "lane-name"))

    vocals_top, vocals_bottom = 254, 268
    body.append(f'<rect x="{left}" y="{vocals_top}" width="{right - left}" height="{vocals_bottom - vocals_top}" rx="2" class="lane-back"/>')
    for start, end in VOCAL_STRETCHES_SECONDS:
        body.append(
            f'<rect x="{energy.number(to_x(start))}" y="{vocals_top}" '
            f'width="{energy.number(to_x(end) - to_x(start))}" height="{vocals_bottom - vocals_top}" class="vocal-fill"/>'
        )
    body.append(energy.text(left + 4, vocals_top + 10, "Vocals", "lane-name"))

    for second in range(0, 121, 20):
        body.append(energy.text(to_x(second), vocals_bottom + 20, energy.minutes_and_seconds(second), "tick", "middle"))

    body += bubble(to_x(48), 92, "Energy 1 at 0:48", energy_bottom - 4)
    body += bubble(to_x(86), 124, "Vocals from 1:04 to 1:44", vocals_top + 6)

    description = (
        "A two minute track drawn as a waveform with two lanes under it, as wide as the waveform. "
        "The Energy lane has a bar for every 4 seconds as tall as its rating from 1 to 10; it falls "
        "to 1 in a breakdown around 40 to 60 seconds and is back at 10 at the drop. The Vocals lane "
        "is filled from 0:08 to 0:38 and from 1:04 to 1:44, and empty in between. Two labels show "
        "what pointing shows: Energy 1 at 0:48 over the breakdown, and Vocals from 1:04 to 1:44."
    )
    return energy.document(width, height, "The Energy and Vocals lanes under a waveform", description, body)


def percentile(values, share):
    ordered = sorted(values)
    return ordered[round((len(ordered) - 1) * share)]


def threshold_chart():
    width = 800
    height = 360
    plot_left, plot_right = 64, 640
    plot_top, plot_bottom = 100, 290
    to_y = energy.scale(-80, -10, plot_bottom, plot_top)
    reference = percentile(WINDOW_LEVELS_DB, LEVEL_REFERENCE_SHARE)
    threshold = max(reference - LEVEL_RELATIVE_DB, LEVEL_FLOOR_DB)
    count = len(WINDOW_LEVELS_DB)
    step = (plot_right - plot_left) / count
    body = [
        energy.text(24, 36, "How the vocals lane decides", "title"),
        energy.text(24, 58, "The level of the vocals stem in each window, against the line 20 dB under the track's own vocal level.", "subtitle"),
    ]
    for level_tick in (-80, -60, -40, -20):
        body.append(energy.line(plot_left, to_y(level_tick), plot_right, to_y(level_tick), "grid"))
        body.append(energy.text(plot_left - 10, to_y(level_tick) + 4, str(level_tick), "tick", "end"))
    body.append(energy.line(plot_left, plot_bottom, plot_right, plot_bottom, "axis"))
    for index, level in enumerate(WINDOW_LEVELS_DB):
        css_class = "level-voice" if level >= threshold else "level-fill"
        top = to_y(level)
        body.append(
            f'<rect x="{energy.number(plot_left + index * step + 1)}" y="{energy.number(top)}" '
            f'width="{energy.number(step - 2)}" height="{energy.number(plot_bottom - top)}" class="{css_class}"/>'
        )
    body.append(energy.line(plot_left, to_y(threshold), plot_right, to_y(threshold), "threshold"))
    body.append(energy.text(plot_right + 10, to_y(threshold) - 4, "Threshold", "threshold-label"))
    body.append(energy.text(plot_right + 10, to_y(threshold) + 13, f"{threshold} dB", "threshold-label"))
    body.append(energy.text((plot_left + plot_right) / 2, plot_bottom + 24, "Window, every 4 seconds from the start of the track", "label", "middle"))
    body.append(energy.text(24, plot_top - 12, "dB below full scale", "label"))

    description = (
        "Bar chart of the level of the vocals stem of one separated track in each 8 second window, "
        "in dB below full scale. Windows with vocals sit between about -16 and -31 dB; windows without "
        "sit between about -69 and -79 dB. A threshold line at "
        f"{threshold} dB, 20 dB under the level the loudest tenth of the windows reach, falls in the "
        "empty space between them, so the windows above it are drawn as vocals."
    )
    return energy.document(width, height, "Level of the vocals stem per window and the threshold", description, body)


CHARTS = {
    "lanes.svg": lanes_chart,
    "vocal-threshold.svg": threshold_chart,
}


def main():
    for file_name, build in CHARTS.items():
        (OUTPUT_DIRECTORY / file_name).write_text(build(), encoding="utf-8")


if __name__ == "__main__":
    main()
