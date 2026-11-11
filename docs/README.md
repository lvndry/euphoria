# Documentation sources

These Markdown files are the source of the Euphoria documentation website. Read them on GitHub, or start at [index.md](index.md).

Layout, which becomes the site navigation:

| Folder | Contents |
| --- | --- |
| `getting-started/` | Installation and a first run |
| `guides/` | Task focused walkthroughs |
| `concepts/` | Background: keys, the Camelot wheel, energy, audio quality, accuracy |
| `reference/` | Exhaustive details: commands, formats |
| `development/` | Building, architecture, benchmarks |

Conventions:

- Every page starts with front matter holding a `title` and a one sentence `description`.
- Links between pages are relative Markdown links, such as `../reference/cli.md`, so they work both on GitHub and on the site.
- Links to files outside `docs/` use full GitHub URLs, since those files are not part of the site.
- One topic per page. Reference pages describe behaviour; guides show how to get something done.
