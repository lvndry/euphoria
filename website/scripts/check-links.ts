/**
 * Scans dist/ and fails when an internal link points at a file the build did not
 * make, at a #fragment its page has no element for, or outside /euphoria where
 * the site is served, and when an absolute URL to the site's origin (canonical
 * links, Open Graph tags, JSON-LD, the sitemap) points outside /euphoria or at
 * a file the build did not make.
 */
import { readdir, readFile } from "node:fs/promises";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";

import { SITE_BASE, SITE_ORIGIN } from "../site";

const distRoot = join(fileURLToPath(new URL("..", import.meta.url)), "dist");

async function collectFiles(folder: string): Promise<string[]> {
  const entries = await readdir(folder, { withFileTypes: true });
  const nested = await Promise.all(
    entries.map((entry) => (entry.isDirectory() ? collectFiles(join(folder, entry.name)) : Promise.resolve([join(folder, entry.name)]))),
  );
  return nested.flat();
}

const files = await collectFiles(distRoot);
const produced = new Set(files.map((file) => `/${relative(distRoot, file).split("\\").join("/")}`));

/** The file a route serves, or undefined when the build made none. */
function resolveRoute(route: string): string | undefined {
  const clean = route.replace(/\/+$/, "");
  const candidates = [clean, `${clean}.html`, `${clean}/index.html`, clean === "" ? "/index.html" : `${clean}.html`];
  return candidates.find((candidate) => produced.has(candidate));
}

const idCache = new Map<string, Set<string>>();
async function idsOf(path: string): Promise<Set<string>> {
  const cached = idCache.get(path);
  if (cached) return cached;
  const html = await readFile(join(distRoot, path), "utf8");
  const ids = new Set([...html.matchAll(/\sid="([^"]+)"/g)].map((match) => match[1] ?? ""));
  idCache.set(path, ids);
  return ids;
}

const broken: string[] = [];
const pages = files.filter((file) => file.endsWith(".html"));
for (const file of pages) {
  const html = await readFile(file, "utf8");
  const ownPath = `/${relative(distRoot, file)}`;
  const links = [...html.matchAll(/(?:href|src)="((?:\/|#)[^"]*)"/g)].map((match) => match[1] ?? "");
  for (const link of new Set(links)) {
    const [withQuery = "", fragment] = link.split("#");
    const sitePath = withQuery.split("?")[0] ?? "";
    if (sitePath !== "" && sitePath !== SITE_BASE && !sitePath.startsWith(`${SITE_BASE}/`)) {
      broken.push(`${relative(distRoot, file)} → ${link} (outside ${SITE_BASE})`);
      continue;
    }
    const route = sitePath === "" ? "" : sitePath.slice(SITE_BASE.length) || "/";
    if (route.startsWith("/pagefind/")) continue;
    const target = route === "" ? ownPath : resolveRoute(decodeURI(route));
    if (target === undefined) {
      broken.push(`${relative(distRoot, file)} → ${link}`);
      continue;
    }
    if (fragment === undefined || fragment.length === 0 || !target.endsWith(".html")) continue;
    if (!(await idsOf(target)).has(decodeURIComponent(fragment))) {
      broken.push(`${relative(distRoot, file)} → ${link} (no element with id "${fragment}")`);
    }
  }
}

const absolute = new RegExp(`${SITE_ORIGIN.replaceAll(".", "\\.")}(/[^"'\\s<>)]*)?`, "g");
for (const file of files.filter((candidate) => /\.(html|xml|txt|json)$/.test(candidate))) {
  const text = await readFile(file, "utf8");
  for (const match of text.matchAll(absolute)) {
    const path = match[1] ?? "";
    if (path !== SITE_BASE && !path.startsWith(`${SITE_BASE}/`)) {
      broken.push(`${relative(distRoot, file)} → ${match[0]} (absolute URL outside ${SITE_BASE})`);
      continue;
    }
    const route = (path.slice(SITE_BASE.length) || "/").split("#")[0] ?? "/";
    if (resolveRoute(decodeURI(route)) === undefined) {
      broken.push(`${relative(distRoot, file)} → ${match[0]} (no such file in the build)`);
    }
  }
}

if (broken.length > 0) {
  console.error(`${broken.length} broken link(s):`);
  for (const line of broken) console.error(`  ${line}`);
  process.exit(1);
}
console.log(`checked ${pages.length} pages: no broken links, anchors or URLs outside ${SITE_BASE}`);
