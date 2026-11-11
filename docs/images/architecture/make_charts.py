# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Writes the diagrams of docs/development/architecture.md as SVG files next
to this script.

Run it with any Python 3 from the repository root:

    python3 docs/images/architecture/make_charts.py

The crates and their dependencies come from the Cargo.toml of each crate, and
the modules from crates/euphoria-core/src/lib.rs. Change them and the data
below together.
"""

from pathlib import Path
from xml.sax.saxutils import escape

OUTPUT_DIRECTORY = Path(__file__).resolve().parent

# The workspace members that depend on euphoria-core, what each is, and the
# main libraries each adds, from its Cargo.toml.
FRONT_ENDS = [
    ("euphoria-cli", "The euphoria command", "clap, indicatif, Rayon"),
    ("euphoria-desktop", "The desktop app", "GPUI, rodio, Rayon"),
    ("euphoria-bench", "Accuracy benchmark, not shipped", "cpu-time, Rayon"),
]

# The public modules of crates/euphoria-core/src/lib.rs, except error,
# grouped by what they do.
CORE_MODULE_GROUPS = [
    ("Find and decode", ["library", "audio", "audio_stream", "track_name"]),
    ("Analyze", ["analysis", "detect", "key", "confidence", "tempo_range", "energy", "quality"]),
    ("Keep and share", ["cache", "correction", "playlists", "harmony", "tags", "export"]),
]

# The libraries euphoria-core builds on, from the [dependencies] of
# crates/euphoria-core/Cargo.toml and its build.rs, with a second line saying
# what for. Tags are read and written by the crate's own code.
CORE_LIBRARIES = [
    ("libkeyfinder", "key, C++, optional"),
    ("stratum-dsp", "tempo"),
    ("Symphonia", "decoding"),
    ("blake3", "content hashes for the cache"),
    ("rusqlite", "the cache, SQLite"),
    ("ebur128, rustfft", "loudness, spectra"),
]

STYLE = """
.surface { fill: #fcfcfb; stroke: rgba(11,11,11,0.10); }
.title { fill: #0b0b0b; font-size: 16px; font-weight: 600; }
.subtitle { fill: #52514e; font-size: 13px; }
.label { fill: #52514e; font-size: 13px; }
.small { fill: #52514e; font-size: 12px; }
.name { fill: #0b0b0b; font-size: 14px; font-weight: 600; }
.group { fill: #0b0b0b; font-size: 13px; font-weight: 600; }
.module { fill: #52514e; font-size: 12px; font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace; }
.box { fill: #fcfcfb; stroke: #c3c2b7; stroke-width: 1; }
.box-optional { stroke-dasharray: 4 3; }
.core { fill: #2a78d6; fill-opacity: 0.08; stroke: #2a78d6; stroke-width: 2; }
.arrow { stroke: #898781; stroke-width: 1.5; fill: none; }
.arrow-head { fill: #898781; }
@media (prefers-color-scheme: dark) {
  .surface { fill: #1a1a19; stroke: rgba(255,255,255,0.10); }
  .title, .name, .group { fill: #f0efec; }
  .subtitle, .label, .small, .module { fill: #c3c2b7; }
  .box { fill: #1a1a19; stroke: #383835; }
  .core { fill: #3987e5; fill-opacity: 0.12; stroke: #3987e5; }
  .arrow { stroke: #898781; }
  .arrow-head { fill: #898781; }
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


def box(left, top, width, height, css_class="box"):
    return (
        f'<rect x="{number(left)}" y="{number(top)}" width="{number(width)}" '
        f'height="{number(height)}" rx="8" class="{css_class}"/>'
    )


def arrow_down(x, top, bottom):
    head = 7
    return "\n".join(
        [
            f'<line x1="{number(x)}" y1="{number(top)}" x2="{number(x)}" y2="{number(bottom - head)}" class="arrow"/>',
            f'<path d="M{number(x - 5)},{number(bottom - head)} L{number(x + 5)},{number(bottom - head)} '
            f'L{number(x)},{number(bottom)} Z" class="arrow-head"/>',
        ]
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


def crates_diagram():
    width = 800
    margin = 24
    gap = 16
    title = "The crates of the workspace"
    body = [
        text(margin, 36, title, "title"),
        text(margin, 58, "Every front end depends on euphoria-core and none depends on another.", "subtitle"),
    ]

    front_top = 84
    front_height = 72
    front_width = (width - 2 * margin - gap * (len(FRONT_ENDS) - 1)) / len(FRONT_ENDS)
    core_top = front_top + front_height + 40
    for index, (name, purpose, libraries) in enumerate(FRONT_ENDS):
        left = margin + index * (front_width + gap)
        body.append(box(left, front_top, front_width, front_height))
        body.append(text(left + 16, front_top + 26, name, "name"))
        body.append(text(left + 16, front_top + 46, purpose, "label"))
        body.append(text(left + 16, front_top + 63, libraries, "small"))
        body.append(arrow_down(left + front_width / 2, front_top + front_height, core_top - 2))

    longest_group = max(len(modules) for _, modules in CORE_MODULE_GROUPS)
    core_height = 64 + 20 * longest_group + 12
    core_width = width - 2 * margin
    body.append(box(margin, core_top, core_width, core_height, "core"))
    body.append(text(margin + 16, core_top + 26, "euphoria-core", "name"))
    body.append(text(margin + 136, core_top + 26, "All product logic. No user interface, prints nothing.", "label"))
    column_width = (core_width - 32) / len(CORE_MODULE_GROUPS)
    for index, (group, modules) in enumerate(CORE_MODULE_GROUPS):
        column_left = margin + 16 + index * column_width
        body.append(text(column_left, core_top + 56, group, "group"))
        for row, module in enumerate(modules):
            body.append(text(column_left, core_top + 78 + 20 * row, module, "module"))

    container_top = core_top + core_height + 40
    columns = 3
    rows = -(-len(CORE_LIBRARIES) // columns)
    library_height = 52
    library_gap = 8
    container_padding = 12
    container_heading = 28
    container_height = container_heading + rows * library_height + (rows - 1) * library_gap + container_padding
    body.append(box(margin, container_top, core_width, container_height))
    body.append(text(margin + 16, container_top + 22, "The main libraries euphoria-core builds on", "label"))
    body.append(arrow_down(width / 2, core_top + core_height, container_top - 2))
    library_width = (core_width - 2 * container_padding - library_gap * (columns - 1)) / columns
    for index, (name, purpose) in enumerate(CORE_LIBRARIES):
        row, column = divmod(index, columns)
        left = margin + container_padding + column * (library_width + library_gap)
        top = container_top + container_heading + row * (library_height + library_gap)
        css_class = "box box-optional" if "optional" in purpose else "box"
        body.append(box(left, top, library_width, library_height, css_class))
        body.append(text(left + 12, top + 22, name, "group"))
        body.append(text(left + 12, top + 40, purpose, "small"))

    height = container_top + container_height + 24
    front_end_names = ", ".join(name for name, _, _ in FRONT_ENDS)
    module_lines = "; ".join(f"{group}: {', '.join(modules)}" for group, modules in CORE_MODULE_GROUPS)
    library_names = ", ".join(name for name, _ in CORE_LIBRARIES)
    description = (
        f"Three crates, {front_end_names}, each point down to euphoria-core, and none to another. "
        f"euphoria-core holds all product logic in its modules. {module_lines}. "
        f"Its main libraries are {library_names}; libkeyfinder, a C++ library, is drawn dashed because "
        "it is behind the optional libkeyfinder feature, on by default."
    )
    return document(width, height, title, description, body)


CHARTS = {
    "crates.svg": crates_diagram,
}


def main():
    for file_name, build in CHARTS.items():
        (OUTPUT_DIRECTORY / file_name).write_text(build(), encoding="utf-8")


if __name__ == "__main__":
    main()
