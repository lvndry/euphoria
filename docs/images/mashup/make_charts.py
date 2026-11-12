"""Writes the chart of docs/guides/mashups.md as an SVG file next to this script.

Run it with any Python 3 from the repository root:

    python3 docs/images/mashup/make_charts.py

The data is what euphoria_core::harmony::mashups_for chooses for a track in
8A with the default options. Change the guide and the data together.
"""

from pathlib import Path
from xml.sax.saxutils import escape

OUTPUT_DIRECTORY = Path(__file__).resolve().parent

# For a track in 8A, the shift deck B needs to reach the key, and the
# relation the shifted key has to 8A. 0 is no shift, and "no fit" is a key
# more than two semitones away, which does not layer at all.
SHIFTS = {
    "1A": ("+1", "Same"),
    "2A": ("-1", "Adjacent"),
    "3A": ("-1", "Same"),
    "4A": ("-1", "Adjacent"),
    "5A": ("+2", "Adjacent"),
    "6A": ("+2", "Same"),
    "7A": ("0", "Adjacent"),
    "8A": ("0", "Same"),
    "9A": ("0", "Adjacent"),
    "10A": ("-2", "Same"),
    "11A": ("-2", "Adjacent"),
    "12A": ("+1", "Adjacent"),
    "1B": ("+1", "Relative"),
    "2B": ("no fit", ""),
    "3B": ("-1", "Relative"),
    "4B": ("no fit", ""),
    "5B": ("no fit", ""),
    "6B": ("+2", "Relative"),
    "7B": ("no fit", ""),
    "8B": ("0", "Relative"),
    "9B": ("no fit", ""),
    "10B": ("-2", "Relative"),
    "11B": ("no fit", ""),
    "12B": ("no fit", ""),
}

CHOSEN = "8A"

STYLE = """
.surface { fill: #fcfcfb; stroke: rgba(11,11,11,0.10); }
.title { fill: #0b0b0b; font-size: 16px; font-weight: 600; }
.subtitle { fill: #52514e; font-size: 13px; }
.label { fill: #52514e; font-size: 13px; }
.value { fill: #0b0b0b; font-size: 13px; font-weight: 600; }
.tick { fill: #898781; font-size: 12px; font-variant-numeric: tabular-nums; }
.grid { stroke: #e1e0d9; stroke-width: 1; fill: none; }
.axis { stroke: #c3c2b7; stroke-width: 1; fill: none; }
.fill-0 { fill: #2a78d6; fill-opacity: 0.88; }
.fill-1 { fill: #2a78d6; fill-opacity: 0.55; }
.fill-2 { fill: #2a78d6; fill-opacity: 0.28; }
.fill-3 { fill: #e1e0d9; }
.chosen { stroke: #0b0b0b; stroke-width: 2.5; }
.on-strong { fill: #ffffff; }
.on-weak { fill: #0b0b0b; }
.corner { font-size: 10px; font-weight: 600; fill: #ffffff; }
@media (prefers-color-scheme: dark) {
  .surface { fill: #1a1a19; stroke: rgba(255,255,255,0.10); }
  .title, .value, .on-weak { fill: #f0efec; }
  .subtitle, .label { fill: #c3c2b7; }
  .grid { stroke: #2c2c2a; }
  .axis { stroke: #383835; }
  .fill-0, .fill-1, .fill-2 { fill: #3987e5; }
  .fill-3 { fill: #2c2c2a; }
  .chosen { stroke: #f0efec; }
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


def rect(left, top, width, height, css_class):
    return (
        f'<rect x="{number(left)}" y="{number(top)}" width="{number(width)}" '
        f'height="{number(height)}" rx="6" class="{css_class}"/>'
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


def shift_label(shift):
    if shift == "0":
        return "no shift"
    if shift == "no fit":
        return "no fit"
    if shift.startswith("-"):
        return "\u2212" + shift[1:]
    return shift


def shift_fill(shift):
    if shift == "0":
        return "fill-0"
    if shift == "no fit":
        return "fill-3"
    if abs(int(shift)) == 1:
        return "fill-1"
    return "fill-2"


def shift_text(shift):
    # Light text on the two strong fills, dark text on the two pale ones.
    if shift in ("0", "+1", "-1"):
        return "on-strong"
    return "on-weak"


def shift_reach_chart():
    width = 820
    cell_width = 62
    cell_height = 54
    cell_gap = 2
    rows = [
        [f"{n}A" for n in range(1, 13)],
        [f"{n}B" for n in range(1, 13)],
    ]
    grid_width = cell_width * 12 + cell_gap * 11
    grid_left = (width - grid_width) / 2
    row_gap = 10
    grid_top = 92
    height = grid_top + cell_height * 2 + row_gap + 82
    body = [
        text(24, 36, "Which keys layer over a track in 8A", "title"),
        text(24, 58, "Shift needed on deck B to reach the same, relative or adjacent key, at most two semitones.", "subtitle"),
    ]
    for row, keys in enumerate(rows):
        top = grid_top + row * (cell_height + row_gap)
        for column, key in enumerate(keys):
            shift, _ = SHIFTS[key]
            left = grid_left + column * (cell_width + cell_gap)
            fill = shift_fill(shift)
            classes = fill + (" chosen" if key == CHOSEN else "")
            body.append(rect(left, top, cell_width, cell_height, classes))
            body.append(text(left + cell_width / 2, top + 22, key, "value " + shift_text(shift), "middle"))
            body.append(text(left + cell_width / 2, top + 41, shift_label(shift), shift_text(shift), "middle"))
            if key == CHOSEN:
                body.append(text(left + cell_width - 5, top + 13, "A", "corner", "end"))

    legend_items = [
        ("fill-0", "No shift"),
        ("fill-1", "1 semitone"),
        ("fill-2", "2 semitones"),
        ("fill-3", "Does not layer"),
    ]
    swatch = 14
    item_gaps = (76, 96, 102, 116)
    total = sum(gaps for gaps in item_gaps) + swatch * 4 + 8 * 3
    x = (width - total) / 2
    legend_top = grid_top + cell_height * 2 + row_gap + 24
    for (fill, name), gap in zip(legend_items, item_gaps):
        body.append(rect(x, legend_top, swatch, swatch, fill))
        body.append(text(x + swatch + 8, legend_top + 11.5, name, "label"))
        x += gap

    description = (
        "Grid of the 24 keys for a track in 8A, minors in the top row and majors in the "
        "bottom row. Blue cells layer: without a shift 7A, 8A, 9A and 8B; one semitone "
        "reaches 1A, 2A, 3A, 4A, 12A, 1B and 3B; two semitones reach 5A, 6A, 10A, 11A, 6B "
        "and 10B. The grey cells, 2B, 4B, 5B, 7B, 9B, 11B and 12B, are more than two "
        "semitones away and do not layer. 8A, the chosen track, has a dark outline and an "
        "A in its corner."
    )
    return document(width, height, "Which keys layer over a track in 8A", description, body)


CHARTS = {
    "shift-reach.svg": shift_reach_chart,
}


def main():
    for file_name, build in CHARTS.items():
        (OUTPUT_DIRECTORY / file_name).write_text(build(), encoding="utf-8")


if __name__ == "__main__":
    main()
