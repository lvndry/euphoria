"""Writes the note strip illustration of docs/concepts/accuracy.md as an SVG file
next to this script.

Run it with any Python 3 from the repository root:

    python3 docs/images/notes/make_charts.py

The levels are made up to show how the strip reads: a track in A minor, whose
loudest notes are A, E and C. They are not a measurement.
"""

from pathlib import Path
from xml.sax.saxutils import escape

OUTPUT_DIRECTORY = Path(__file__).resolve().parent

NOTE_NAMES = ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"]
LEVELS = [0.78, 0.22, 0.55, 0.2, 0.82, 0.5, 0.24, 0.6, 0.25, 1.0, 0.2, 0.52]
KEYS = [
    ("A minor (8A)", 9, [0, 2, 3, 5, 7, 8, 10]),
    ("C major (8B)", 0, [0, 2, 4, 5, 7, 9, 11]),
]

STYLE = """
.surface { fill: #fcfcfb; stroke: rgba(11,11,11,0.10); }
.title { fill: #0b0b0b; font-size: 16px; font-weight: 600; }
.subtitle { fill: #52514e; font-size: 13px; }
.label { fill: #52514e; font-size: 13px; }
.note { fill: #898781; font-size: 12px; }
.note-lit { fill: #0b0b0b; font-size: 12px; font-weight: 600; }
.lit { fill: #2a78d6; }
.unlit { fill: #d9d8d2; }
.tonic { fill: none; stroke: #0b0b0b; stroke-width: 1.5; }
@media (prefers-color-scheme: dark) {
  .surface { fill: #1a1a19; stroke: rgba(255,255,255,0.10); }
  .title, .note-lit { fill: #f0efec; }
  .subtitle, .label { fill: #c3c2b7; }
  .note { fill: #898781; }
  .lit { fill: #8fbcf2; }
  .unlit { fill: #383835; }
  .tonic { stroke: #f0efec; }
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


def strip(left, top, key_name, tonic, steps):
    lit = {(tonic + step) % 12 for step in steps}
    column = 28
    bar_width = 20
    bar_area = 80
    body = [text(left, top, key_name, "label")]
    for pitch_class, level in enumerate(LEVELS):
        x = left + pitch_class * column
        height = max(2.0, level * bar_area)
        y = top + 16 + bar_area - height
        css = "lit" if pitch_class in lit else "unlit"
        body.append(
            f'<rect x="{number(x)}" y="{number(y)}" width="{bar_width}" '
            f'height="{number(height)}" rx="4" class="{css}"/>'
        )
        if pitch_class == tonic:
            body.append(
                f'<rect x="{number(x - 3)}" y="{number(y - 3)}" width="{bar_width + 6}" '
                f'height="{number(height + 6)}" rx="6" class="tonic"/>'
            )
        note_class = "note-lit" if pitch_class in lit else "note"
        body.append(text(x + bar_width / 2, top + 16 + bar_area + 20, NOTE_NAMES[pitch_class], note_class, "middle"))
    return body


def relative_keys_chart():
    width = 800
    height = 230
    title = "A minor and C major light the same notes"
    body = [
        text(24, 36, title, "title"),
        text(24, 58, "One track, two readings. The lit bars are the notes of the key; the outlined bar is its tonic.", "subtitle"),
    ]
    for index, (key_name, tonic, steps) in enumerate(KEYS):
        body.extend(strip(24 + index * 392, 96, key_name, tonic, steps))
    description = (
        "Two copies of the same note strip of a made up track whose loudest notes are A, E and C. "
        "The first lights the notes of A minor, the second those of C major: the same seven notes, "
        "A B C D E F G. Only the tonic differs, A or C. Here A is the tallest bar, which points to A minor."
    )
    return document(width, height, title, description, body)


def main():
    (OUTPUT_DIRECTORY / "relative-keys.svg").write_text(relative_keys_chart(), encoding="utf-8")


if __name__ == "__main__":
    main()
