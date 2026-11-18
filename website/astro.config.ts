import { fileURLToPath } from "node:url";

import sitemap from "@astrojs/sitemap";
import { defineConfig } from "astro/config";

import { SITE_BASE, SITE_ORIGIN } from "./site";
import { remarkDocsLinks } from "./src/lib/remark-docs-links";

const docsRoot = fileURLToPath(new URL("../docs", import.meta.url));
const repositoryRoot = fileURLToPath(new URL("..", import.meta.url));

export default defineConfig({
  site: SITE_ORIGIN,
  base: SITE_BASE,
  trailingSlash: "never",
  build: {
    format: "file",
  },
  integrations: [
    sitemap({
      serialize: (item) => ({ ...item, url: item.url.replace(/index\.html$/, "").replace(/\.html$/, "") }),
    }),
  ],
  markdown: {
    remarkPlugins: [[remarkDocsLinks, { docsRoot, repoUrl: "https://github.com/lvndry/euphoria", base: SITE_BASE }]],
  },
  vite: {
    // The docs and their images, and the logo, live outside website/.
    server: { fs: { allow: [repositoryRoot] } },
  },
});
