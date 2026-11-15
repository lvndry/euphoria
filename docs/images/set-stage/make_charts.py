"""Writes the chart of docs/concepts/set-stage.md as an SVG file next to this script.

Run it with any Python 3 from the repository root:

    python3 docs/images/set-stage/make_charts.py

Every number comes from docs/concepts/set-stage.md. Change the page and the data
below together.
"""

from pathlib import Path
from xml.sax.saxutils import escape

OUTPUT_DIRECTORY = Path(__file__).resolve().parent

# "How well it works": the stage Euphoria gave to each track of the four
# biggest genre folders, out of fold, by the folder the DJ filed it in. Each
# row counts the tracks given warm-up, build and peak.
FOLDER_ROWS = [
    ("warmup", [17, 1, 2]),
    ("build", [39, 111, 8]),
    ("peak", [4, 25, 284]),
    ("closing", [7, 36, 27]),
]
STAGE_NAMES = ["Warm-up", "Build", "Peak"]

STYLE = """
.surface { fill: #fcfcfb; stroke: rgba(11,11,11,0.10); }
.title { fill: #0b0b0b; font-size: 16px; font-weight: 600; }
.subtitle { fill: #52514e; font-size: 13px; }
.label { fill: #52514e; font-size: 13px; }
.value { fill: #0b0b0b; font-size: 12px; font-weight: 600; font-variant-numeric: tabular-nums; }
.value-light { fill: #ffffff; font-size: 12px; font-weight: 600; font-variant-numeric: tabular-nums; }
.tick { fill: #898781; font-size: 12px; font-variant-numeric: tabular-nums; }
.axis { stroke: #c3c2b7; stroke-width: 1; fill: none; }
.stage-0 { fill: #b9d3f2; }
.stage-1 { fill: #6aa3e8; }
.stage-2 { fill: #2a78d6; }
.gap { stroke: #fcfcfb; stroke-width: 2; }
@media (prefers-color-scheme: dark) {
  .surface { fill: #1a1a19; stroke: rgba(255,255,255,0.10); }
  .title, .value { fill: #f0efec; }
  .subtitle, .label { fill: #c3c2b7; }
  .axis { stroke: #383835; }
  .stage-0 { fill: #1f4a82; }
  .stage-1 { fill: #3987e5; }
  .stage-2 { fill: #8fbcf2; }
  .value-light { fill: #f0efec; }
  .gap { stroke: #1a1a19; }
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


def folder_chart():
    width = 800
    row_height = 48
    chart_top = 128
    height = chart_top + row_height * len(FOLDER_ROWS) + 24
    left = 120
    right = 760
    body = [
        text(24, 36, "The stage Euphoria gives to tracks of each folder", "title"),
        text(24, 58, "Share of the tracks of each folder given each stage, tested on genres the weights never saw.", "subtitle"),
    ]
    legend_x = left
    for index, name in enumerate(STAGE_NAMES):
        body.append(f'<rect x="{legend_x}" y="82" width="14" height="14" rx="3" class="stage-{index}"/>')
        body.append(text(legend_x + 22, 94, name, "label"))
        legend_x += 110
    for row, (folder, counts) in enumerate(FOLDER_ROWS):
        total = sum(counts)
        top = chart_top + row * row_height
        middle = top + 14
        body.append(text(left - 14, middle + 5, folder, "label", "end"))
        body.append(text(left - 14, middle + 22, f"{total} tracks", "tick", "end"))
        x = left
        for index, count in enumerate(counts):
            segment = (right - left) * count / total
            body.append(
                f'<rect x="{number(x)}" y="{top}" width="{number(segment)}" height="28" '
                f'class="stage-{index} gap"/>'
            )
            if segment >= 34:
                css = "value-light" if index != 0 else "value"
                body.append(text(x + segment / 2, middle + 5, f"{round(100 * count / total)}%", css, "middle"))
            x += segment
    description = (
        "Stacked bars of the share of tracks given warm-up, build and peak, by the folder "
        "they were filed in. Of the tracks in the warmup folder 85 percent get warm-up. "
        "Of build, 70 percent get build. Of peak, 91 percent get peak. Tracks filed as "
        "closing spread over build 51 percent and peak 39 percent, which is why there is "
        "no closing stage."
    )
    return document(width, height, "The stage Euphoria gives to tracks of each folder", description, body)


def main():
    (OUTPUT_DIRECTORY / "folders-by-stage.svg").write_text(folder_chart(), encoding="utf-8")


if __name__ == "__main__":
    main()
