import path from "node:path";

interface MarkdownNode {
  type?: string;
  url?: string;
  children?: MarkdownNode[];
}

interface Options {
  /** Absolute path to the repository's docs/ folder. */
  docsRoot: string;
  /** GitHub repository URL, with no trailing slash. */
  repoUrl: string;
  /** Branch that GitHub links point at. Defaults to "main". */
  branch?: string;
  /** Path the site is served under, such as "/euphoria", put before every site route. */
  base?: string;
}

const EXTERNAL = /^[a-z][a-z0-9+.-]*:/i;

function walk(node: MarkdownNode, visit: (node: MarkdownNode) => void): void {
  visit(node);
  for (const child of node.children ?? []) walk(child, visit);
}

/**
 * The docs are written for GitHub: relative links between .md files, and
 * relative links up into the repository. On the site, links between pages
 * become routes and every other file points at GitHub, so no link written for
 * GitHub breaks here. Images are left to Astro, which bundles them.
 */
export function remarkDocsLinks(options: Options) {
  const { docsRoot, repoUrl, branch = "main", base = "" } = options;
  const repoRoot = path.dirname(docsRoot);
  const route = (url: string): string =>
    base === "" || url === base || url.startsWith(`${base}/`) || url.startsWith(`${base}#`) ? url : `${base}${url}`;

  const rewrite = (url: string, fromDir: string): string => {
    if (url.startsWith("/") && !url.startsWith("//")) return route(url);
    if (url === "" || url.startsWith("#") || url.startsWith("/") || EXTERNAL.test(url)) return url;
    const [target = "", anchor] = url.split("#", 2);
    const suffix = anchor ? `#${anchor}` : "";
    const resolved = path.resolve(fromDir, decodeURI(target));
    const inDocs = path.relative(docsRoot, resolved);
    if (!inDocs.startsWith("..") && inDocs.endsWith(".md")) {
      const slug = inDocs.slice(0, -".md".length).split(path.sep).join("/").replace(/\/index$/, "").replace(/^index$/, "");
      return route(slug === "" ? `/docs${suffix}` : `/docs/${slug}${suffix}`);
    }
    const inRepo = path.relative(repoRoot, resolved);
    if (inRepo.startsWith("..")) return url;
    const kind = target.endsWith("/") ? "tree" : "blob";
    return `${repoUrl}/${kind}/${branch}/${inRepo.split(path.sep).join("/")}${suffix}`;
  };

  return (tree: MarkdownNode, file: { path?: string }): void => {
    const fromDir = file.path ? path.dirname(file.path) : docsRoot;
    walk(tree, (node) => {
      if ((node.type === "link" || node.type === "definition") && typeof node.url === "string") {
        node.url = rewrite(node.url, fromDir);
      }
    });
  };
}
