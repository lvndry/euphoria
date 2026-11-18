# Website

The Euphoria website, built with [Astro](https://astro.build) and published by GitHub Pages at https://lvndry.github.io/euphoria/.

| Route | Source | What it shows |
| --- | --- | --- |
| `/` | `src/pages/index.astro` | What Euphoria is, the app, key, tempo and energy, the key model in brief, a tour, install and questions |
| `/features` | `src/pages/features.astro` | Every area of the desktop app, one tab each, with a screenshot, what it does and a link to its guide |
| `/mixing` | `src/pages/mixing.astro` | The Camelot wheel, every key move as a curve, an interactive mixer, the key model's accuracy and the mixing tools in the app |
| `/docs/...` | `../docs/**/*.md` | The documentation, one page per Markdown file, with a sidebar and search |
| `/og/...png` | `src/pages/og/[...id].png.ts` | A 1200 by 630 social image for every page |

## Work on it

```
cd website
bun install
bun run dev          # a live preview at http://localhost:4321/euphoria/
bun run build        # the site in dist/, and its search index
bun run check        # type-check the pages and scripts
bun run check-links  # after a build: every link, anchor and absolute URL resolves under /euphoria
```

CI runs the build and both checks on every pull request that touches the site, the docs or the logo, and `.github/workflows/website.yml` deploys `main`. `astro check` does not support TypeScript 7 yet, so `typescript` stays on version 6 until it does.

## How it fits together

- `site.ts` names the address, `https://lvndry.github.io` and the base `/euphoria`. `src/lib/url.ts` makes every route and absolute URL from them, and `check-links` fails on anything outside the base.
- `src/layouts/Base.astro` holds the head of every page: title, description, canonical URL, Open Graph and Twitter tags, JSON-LD, the nav and the footer. `src/layouts/Docs.astro` adds the docs sidebar, search and previous and next links.
- The documentation is `../docs`, read as a content collection. Sections follow the table in `docs/README.md`, and pages within a section follow the order `docs/index.md` links them, so a new page appears without a list to update. Links between `.md` files become routes and links to other files point at GitHub (`src/lib/remark-docs-links.ts`).
- Screenshots and charts come from `../docs/images` through `src/lib/images.ts`, the one set the docs and the site share; a page naming an image that does not exist fails the build. Make the app's screenshots again with `docs/images/desktop-app/make_screenshots.py`.
- The curves are the logo's own: the Lissajous figure of two notes, in `src/lib/curves.ts`, drawn by `src/scripts/site.ts` with the timing of `apps/euphoria-desktop/src/logo_motion.rs`.
- Social images are drawn at build time from SVG with Instrument Sans by `src/lib/og.ts`, with no browser.

## Claims

Every number on the pages comes from the repository: the key accuracy from `docs/concepts/accuracy.md`, the features and their limits from the guides. When a result changes there, change it here too.
