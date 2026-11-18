/**
 * Routes, titles and the sidebar of the documentation, built from the docs
 * collection. Sections follow the table in docs/README.md; within a section,
 * pages follow the order docs/index.md links them, and pages it does not link
 * follow alphabetically, so a new page appears without a list to update.
 */
import type { CollectionEntry } from "astro:content";

import { withBase } from "./url";

export type DocsEntry = CollectionEntry<"docs">;

export const SECTIONS: ReadonlyArray<{ dir: string; label: string }> = [
  { dir: "getting-started", label: "Getting started" },
  { dir: "guides", label: "Guides" },
  { dir: "concepts", label: "Concepts" },
  { dir: "reference", label: "Reference" },
  { dir: "development", label: "Development" },
];

/** "guides/write-tags" → "guides/write-tags"; "development/index" → "development"; "index" → "". */
export function routeSlugFor(id: string): string {
  if (id === "index") return "";
  return id.endsWith("/index") ? id.slice(0, -"/index".length) : id;
}

export function routeFor(id: string): string {
  const slug = routeSlugFor(id);
  return withBase(slug === "" ? "/docs" : `/docs/${slug}`);
}

export const sectionOf = (id: string): string => id.split("/")[0] ?? "";

/** Route of a docs page's social image, named like its page: "/og/docs/guides/write-tags.png", "/og/docs/development.png". */
export const ogImageFor = (id: string): string => `/og/docs/${routeSlugFor(id) || "index"}.png`;

export interface SidebarItem {
  title: string;
  route: string;
  id: string;
}

export interface SidebarSection {
  label: string;
  route: string | undefined;
  items: SidebarItem[];
}

/** The ids docs/index.md links, in the order it links them. */
function linkedOrder(entries: DocsEntry[]): string[] {
  const index = entries.find((entry) => entry.id === "index");
  const links = [...(index?.body ?? "").matchAll(/\]\(([a-z0-9/-]+)\.md(?:#[^)]*)?\)/g)];
  return [...new Set(links.map((match) => match[1] ?? ""))];
}

export function buildSidebar(entries: DocsEntry[]): SidebarSection[] {
  const order = linkedOrder(entries);
  const rank = (id: string): number => {
    const position = order.indexOf(id);
    return position === -1 ? order.length : position;
  };
  const sections: SidebarSection[] = [];
  for (const { dir, label } of SECTIONS) {
    const bucket = entries.filter((entry) => sectionOf(entry.id) === dir);
    if (bucket.length === 0) continue;
    const indexEntry = bucket.find((entry) => entry.id === `${dir}/index`);
    const rest = bucket
      .filter((entry) => entry !== indexEntry)
      .sort((first, second) => rank(first.id) - rank(second.id) || first.id.localeCompare(second.id));
    sections.push({
      label,
      route: indexEntry ? routeFor(indexEntry.id) : undefined,
      items: rest.map((entry) => ({ title: entry.data.title, route: routeFor(entry.id), id: entry.id })),
    });
  }
  return sections;
}

export interface PrevNext {
  previous: SidebarItem | undefined;
  next: SidebarItem | undefined;
}

export function previousAndNext(sections: SidebarSection[], id: string): PrevNext {
  const flat = sections.flatMap((section) => section.items);
  const position = flat.findIndex((item) => item.id === id);
  if (position < 0) return { previous: undefined, next: undefined };
  return { previous: flat[position - 1], next: flat[position + 1] };
}
