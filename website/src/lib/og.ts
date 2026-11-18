/**
 * Social preview images, rendered at build time: 1200 by 630, the site's paper,
 * the Euphoria mark, the page's title and one line, and art for the page (the
 * logo's curve, the app, or the interval curves). resvg draws them from SVG with
 * Instrument Sans, so the build needs no browser.
 */
import { existsSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";

import { Resvg } from "@resvg/resvg-js";

import { fullCurve, strokeFor, type Ratio } from "./curves";

const WIDTH = 1200;
const HEIGHT = 630;
const PAPER = "#F6F5F1";
const PANEL = "#EDECE7";
const INK = "#161616";
const SOFT = "#46474B";
const FAINT = "#64656A";

// Resolved through Node so the files are found wherever the bundled build runs from.
const requireFromHere = createRequire(import.meta.url);
const FONT_FILES = [
  requireFromHere.resolve("@expo-google-fonts/instrument-sans/400Regular/InstrumentSans_400Regular.ttf"),
  requireFromHere.resolve("@expo-google-fonts/instrument-sans/600SemiBold/InstrumentSans_600SemiBold.ttf"),
];
for (const fontFile of FONT_FILES) {
  if (!existsSync(fontFile)) throw new Error(`Social image font not found: ${fontFile}; resvg would render no text`);
}

export type OgArt =
  | { kind: "curve"; ratio: Ratio }
  | { kind: "screenshot"; file: string }
  | { kind: "intervals"; curves: ReadonlyArray<{ ratio: Ratio; name: string; hue: number }> };

const escapeXml = (text: string): string =>
  text.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;");

/**
 * Splits text into at most `maxLines` lines of about `perLine` characters, ending the last with … when cut.
 * A line break in the text is kept, so a title can say where it breaks.
 */
function wrap(text: string, perLine: number, maxLines: number): string[] {
  if (text.includes("\n")) return text.split("\n").slice(0, maxLines);
  const lines: string[] = [];
  let current = "";
  for (const word of text.split(" ")) {
    if (current !== "" && `${current} ${word}`.length > perLine) {
      lines.push(current);
      current = word;
    } else {
      current = current === "" ? word : `${current} ${word}`;
    }
  }
  if (current !== "") lines.push(current);
  if (lines.length <= maxLines) return lines;
  const kept = lines.slice(0, maxLines);
  kept[maxLines - 1] = `${(kept[maxLines - 1] ?? "").slice(0, perLine - 1)}…`;
  return kept;
}

/** OKLCH colours of the site's curves, converted to sRGB for resvg. */
function oklch(lightness: number, chroma: number, hueDegrees: number): string {
  const hueRadians = (hueDegrees * Math.PI) / 180;
  const labA = chroma * Math.cos(hueRadians);
  const labB = chroma * Math.sin(hueRadians);
  const longCone = (lightness + 0.3963377774 * labA + 0.2158037573 * labB) ** 3;
  const mediumCone = (lightness - 0.1055613458 * labA - 0.0638541728 * labB) ** 3;
  const shortCone = (lightness - 0.0894841775 * labA - 1.291485548 * labB) ** 3;
  const linear = [
    4.0767416621 * longCone - 3.3077115913 * mediumCone + 0.2309699292 * shortCone,
    -1.2684380046 * longCone + 2.6097574011 * mediumCone - 0.3413193965 * shortCone,
    -0.0041960863 * longCone - 0.7034186147 * mediumCone + 1.707614701 * shortCone,
  ];
  const channels = linear.map((value) => {
    const clamped = Math.min(Math.max(value, 0), 1);
    const encoded = clamped <= 0.0031308 ? 12.92 * clamped : 1.055 * clamped ** (1 / 2.4) - 0.055;
    return Math.round(encoded * 255).toString(16).padStart(2, "0");
  });
  return `#${channels.join("")}`;
}

function art(spec: OgArt): string {
  if (spec.kind === "curve") {
    return `<g transform="translate(700 70) scale(7.4)"><path d="${fullCurve(spec.ratio)}" fill="none" stroke="${INK}" stroke-width="${strokeFor(spec.ratio, 1.15)}" stroke-linecap="round" stroke-linejoin="round"/></g>`;
  }
  if (spec.kind === "screenshot") {
    const data = readFileSync(spec.file).toString("base64");
    return `<rect x="706" y="100" width="560" height="372" rx="16" fill="${PANEL}"/><image x="720" y="114" width="760" height="460" href="data:image/png;base64,${data}" preserveAspectRatio="xMinYMin slice" clip-path="url(#shot)"/>`;
  }
  return spec.curves
    .map(({ ratio, name, hue }, index) => {
      const column = index % 3;
      const row = Math.floor(index / 3);
      const x = 640 + column * 168;
      const y = 112 + row * 196;
      return `<g transform="translate(${x} ${y}) scale(1.8)"><path d="${fullCurve(ratio)}" fill="none" stroke="${oklch(0.58, 0.15, hue)}" stroke-width="${strokeFor(ratio, 2.6)}" stroke-linecap="round" stroke-linejoin="round"/></g><text x="${x + 2}" y="${y + 138}" font-family="Instrument Sans" font-weight="600" font-size="19" fill="${INK}">${escapeXml(name)}</text><text x="${x + 2}" y="${y + 162}" font-family="Instrument Sans" font-size="17" fill="${SOFT}">${ratio[0]} : ${ratio[1]}</text>`;
    })
    .join("");
}

export function renderOgImage(title: string, line: string, spec: OgArt): Buffer {
  const titleLines = wrap(title, title.length > 34 ? 18 : 16, 3);
  const titleSize = titleLines.length > 2 ? 72 : 96;
  const titleTop = 230 - (titleLines.length - 1) * titleSize * 0.42;
  const titleText = titleLines
    .map((text, index) => `<text x="72" y="${titleTop + index * titleSize * 0.95}" font-family="Instrument Sans" font-weight="600" font-size="${titleSize}" letter-spacing="${-titleSize * 0.05}" fill="${INK}">${escapeXml(text)}</text>`)
    .join("");
  const lineTop = titleTop + (titleLines.length - 1) * titleSize * 0.95 + 64;
  const lineText = wrap(line, 34, 3)
    .map((text, index) => `<text x="72" y="${lineTop + index * 38}" font-family="Instrument Sans" font-size="28" fill="${SOFT}">${escapeXml(text)}</text>`)
    .join("");
  const svg = `<svg width="${WIDTH}" height="${HEIGHT}" viewBox="0 0 ${WIDTH} ${HEIGHT}" xmlns="http://www.w3.org/2000/svg">
  <defs><clipPath id="shot"><rect x="720" y="114" width="480" height="344" rx="8"/></clipPath></defs>
  <rect width="${WIDTH}" height="${HEIGHT}" fill="${PAPER}"/>
  <g transform="translate(72 62) scale(0.6)"><path d="${fullCurve([3, 2])}" fill="none" stroke="${INK}" stroke-width="4.2" stroke-linecap="round" stroke-linejoin="round"/></g>
  <text x="124" y="95" font-family="Instrument Sans" font-weight="600" font-size="30" fill="${INK}">Euphoria</text>
  ${titleText}
  ${lineText}
  <text x="72" y="566" font-family="Instrument Sans" font-weight="600" font-size="22" fill="${INK}">lvndry.github.io/euphoria</text>
  <text x="350" y="566" font-family="Instrument Sans" font-size="22" fill="${FAINT}">Free · Open source · Offline</text>
  ${art(spec)}
</svg>`;
  const resvg = new Resvg(svg, { fitTo: { mode: "width", value: WIDTH }, font: { fontFiles: FONT_FILES, loadSystemFonts: false, defaultFontFamily: "Instrument Sans" } });
  return resvg.render().asPng();
}
