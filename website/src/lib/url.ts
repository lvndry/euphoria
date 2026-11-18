import { SITE_BASE, SITE_ORIGIN } from "../../site";

/** A site route under the path the site is served from: "/docs" → "/euphoria/docs", "/" → "/euphoria/". */
export function withBase(route: string): string {
  if (!route.startsWith("/") || route.startsWith("//")) return route;
  if (route === SITE_BASE || route.startsWith(`${SITE_BASE}/`) || route.startsWith(`${SITE_BASE}#`)) return route;
  return route === "/" ? `${SITE_BASE}/` : `${SITE_BASE}${route}`;
}

/** The absolute URL of a site route, for canonical links, Open Graph tags, the sitemap and JSON-LD. */
export function absoluteUrl(route: string): string {
  return new URL(withBase(route), SITE_ORIGIN).href;
}
