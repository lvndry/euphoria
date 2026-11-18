import { fileURLToPath } from "node:url";

import { getCollection } from "astro:content";

import { ogImageFor, SECTIONS, sectionOf, type DocsEntry } from "../../lib/docs";
import { renderOgImage, type OgArt } from "../../lib/og";

interface OgProps {
  title: string;
  line: string;
  art: OgArt;
}

const screenshot = fileURLToPath(new URL("../../../../docs/images/desktop-app/main-window-paper.png", import.meta.url));

/** Each docs section draws its pages with one interval curve, so a section's images belong together. */
const SECTION_CURVES: Record<string, OgArt> = {
  "getting-started": { kind: "curve", ratio: [3, 2] },
  guides: { kind: "curve", ratio: [4, 3] },
  concepts: { kind: "curve", ratio: [6, 5] },
  reference: { kind: "curve", ratio: [5, 4] },
  development: { kind: "curve", ratio: [5, 3] },
};

export async function getStaticPaths() {
  const entries: DocsEntry[] = await getCollection("docs");
  const pages = [
    {
      params: { id: "home" },
      props: { title: "Every track,\nin key.", line: "Key, tempo and energy for your whole collection, on macOS and Linux.", art: { kind: "curve", ratio: [3, 2] } },
    },
    {
      params: { id: "features" },
      props: { title: "Everything\nEuphoria does.", line: "Library, decks, set builder, Live, stems, tags and exports to your DJ software.", art: { kind: "screenshot", file: screenshot } },
    },
    {
      params: { id: "mixing" },
      props: {
        title: "Mix\nin key.",
        line: "Every Camelot move drawn as the curve its two notes trace, and our own key model.",
        art: {
          kind: "intervals",
          curves: [
            { ratio: [1, 1], name: "Same key", hue: 240 },
            { ratio: [3, 2], name: "Fifth", hue: 270 },
            { ratio: [4, 3], name: "Fourth", hue: 210 },
            { ratio: [6, 5], name: "Relative", hue: 240 },
            { ratio: [9, 8], name: "Energy boost", hue: 300 },
            { ratio: [45, 32], name: "Clash", hue: 60 },
          ],
        },
      },
    },
  ] satisfies Array<{ params: { id: string }; props: OgProps }>;
  const docs = entries.map((entry) => {
    const section = SECTIONS.find((candidate) => candidate.dir === sectionOf(entry.id));
    return {
      params: { id: ogImageFor(entry.id).slice('/og/'.length, -'.png'.length) },
      props: {
        title: entry.data.title,
        line: section ? `Euphoria docs · ${section.label}` : "Euphoria docs",
        art: SECTION_CURVES[sectionOf(entry.id)] ?? { kind: "curve", ratio: [3, 2] },
      } satisfies OgProps,
    };
  });
  return [...pages, ...docs];
}

export function GET({ props }: { props: OgProps }): Response {
  return new Response(new Uint8Array(renderOgImage(props.title, props.line, props.art)), { headers: { "Content-Type": "image/png" } });
}
