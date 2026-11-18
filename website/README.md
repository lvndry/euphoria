# Website

The Euphoria website: three static pages with no framework and no build tools beyond Python.

| Page | What it shows |
| --- | --- |
| `index.html` | What Euphoria is, the app, key, tempo and energy, the key model in brief, a tour, install and questions |
| `features.html` | Every area of the desktop app, one tab each, with a screenshot, what it does and a link to its guide |
| `mixing.html` | The Camelot wheel, every key move as a curve, an interactive mixer, the key model's accuracy and the mixing tools in the app |

`assets/site.css` and `assets/site.js` serve the home page and the mixing page, `assets/features.css` and `assets/features.js` the features page. The curves are the logo's own: the Lissajous figure of two notes, drawn tip to tip with the timing of `apps/euphoria-desktop/src/logo_motion.rs`.

## Build and preview

```
python3 scripts/build_website.py
open target/website/index.html
```

The pages name their images as `images/...`. The build copies each one from `docs/images`, so the website and the documentation share one set of screenshots and figures, and copies `assets/logo/favicon.svg`. It fails, naming each one, when a page names an image or a page that does not exist; `scripts/test_build_website.py` runs that check in CI.

## Screenshots

The screenshots of the app come from `docs/images/desktop-app/make_screenshots.py`, on a demo library of GiantSteps previews. To make again only the ones the website added:

```
python3 docs/images/desktop-app/make_screenshots.py focus-view decks set-builder command-palette
```

## Publishing

`.github/workflows/website.yml` builds the site and deploys it to GitHub Pages, at https://lvndry.github.io/euphoria/, on every push to `main` that changes it, its images or the build. Pages is set to deploy from GitHub Actions in the repository settings.

## Claims

Every number on the pages comes from the repository: the key accuracy from `docs/concepts/accuracy.md`, the features and their limits from the guides. When a result changes there, change it here too.
