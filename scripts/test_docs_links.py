"""Checks that the relative links in the README and under docs/ lead to a
file that exists and, when they name a heading, to a heading of that file."""

import pathlib
import re
import tempfile
import unittest

REPOSITORY = pathlib.Path(__file__).resolve().parent.parent
LINK = re.compile(r"\[[^\]]*\]\((?P<target>[^)\s]+)\)")
CODE_BLOCK = re.compile(r"^```.*?^```", re.DOTALL | re.MULTILINE)


def heading_anchors(path):
    """The anchors GitHub gives the headings of a Markdown file."""
    anchors = set()
    counts = {}
    for line in CODE_BLOCK.sub("", path.read_text()).splitlines():
        match = re.match(r"^#{1,6} (.+?)\s*#*\s*$", line)
        if not match:
            continue
        text = re.sub(r"[`*_]", "", match[1].strip().lower())
        anchor = re.sub(r"[^\w\- ]", "", text).replace(" ", "-")
        # GitHub numbers repeated headings: name, name-1, name-2.
        seen = counts.get(anchor, 0)
        counts[anchor] = seen + 1
        anchors.add(anchor if seen == 0 else f"{anchor}-{seen}")
    return anchors


def broken_links(path):
    problems = []
    for match in LINK.finditer(CODE_BLOCK.sub("", path.read_text())):
        target = match["target"]
        if re.match(r"^[a-z]+:", target):
            continue
        file_part, _, anchor = target.partition("#")
        destination = (path.parent / file_part).resolve() if file_part else path
        if not destination.exists():
            problems.append(f"{target}: no such file")
        elif (
            anchor
            and destination.suffix == ".md"
            and anchor not in heading_anchors(destination)
        ):
            problems.append(f"{target}: no such heading")
    return problems


class DocsLinkTests(unittest.TestCase):
    def test_relative_links_lead_somewhere(self):
        files = [REPOSITORY / "README.md", *sorted((REPOSITORY / "docs").rglob("*.md"))]
        self.assertGreater(len(files), 1)
        for path in files:
            with self.subTest(path.relative_to(REPOSITORY).as_posix()):
                self.assertEqual(broken_links(path), [])

    def test_a_link_to_a_missing_file_or_heading_is_reported(self):
        with tempfile.TemporaryDirectory() as folder:
            page = pathlib.Path(folder) / "page.md"
            (pathlib.Path(folder) / "other.md").write_text("## The desktop app\n")
            page.write_text(
                "[good](other.md#the-desktop-app) [heading](other.md#the-app) "
                "[file](missing.md) [web](https://example.com/x.md#y)\n"
                "```\n[in code](missing.md)\n```\n"
            )
            self.assertEqual(
                broken_links(page),
                ["other.md#the-app: no such heading", "missing.md: no such file"],
            )

    def test_the_desktop_guide_links_to_installation_headings_that_exist(self):
        installation = heading_anchors(
            REPOSITORY / "docs" / "getting-started" / "installation.md"
        )
        guide = (REPOSITORY / "docs" / "guides" / "desktop-app.md").read_text()
        for anchor in ("the-desktop-app", "opening-the-app-for-the-first-time"):
            self.assertIn(f"installation.md#{anchor}", guide)
            self.assertIn(anchor, installation)


if __name__ == "__main__":
    unittest.main()
