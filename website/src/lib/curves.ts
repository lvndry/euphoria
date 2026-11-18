/**
 * The logo's curve and its relatives: the Lissajous figure two notes trace on
 * an oscilloscope, x = sin(larger · t), y = cos(smaller · t), drawn in a 64 by 64
 * box. The logo is 3:2, a perfect fifth.
 */

/** Two whole numbers of an interval, larger first, such as [3, 2] for a fifth. */
export type Ratio = readonly [number, number];

export function point(larger: number, smaller: number, turn: number): [number, number] {
  return [32 + 24 * Math.sin(larger * 2 * Math.PI * turn), 32 + 24 * Math.cos(smaller * 2 * Math.PI * turn)];
}

const spanCache = new Map<string, [number, number]>();

/** The stretch of the curve's turn that draws it once: half a turn when the second half retraces the first. */
export function spanFor(larger: number, smaller: number): [number, number] {
  const key = `${larger}:${smaller}`;
  const cached = spanCache.get(key);
  if (cached) return cached;
  let retraces = true;
  for (let step = 0; step <= 200 && retraces; step++) {
    const turn = 0.25 + (0.5 * step) / 200;
    const [firstAcross, firstDown] = point(larger, smaller, turn);
    const [secondAcross, secondDown] = point(larger, smaller, 1.5 - turn);
    if (Math.abs(firstAcross - secondAcross) > 1e-3 || Math.abs(firstDown - secondDown) > 1e-3) retraces = false;
  }
  const span: [number, number] = retraces ? [0.25, 0.75] : [0.25, 1.25];
  spanCache.set(key, span);
  return span;
}

/** SVG path data for the curve from `from` to `to` turns. */
export function curvePath(larger: number, smaller: number, from: number, to: number): string {
  if (to - from <= 0) return "";
  const [start, end] = spanFor(larger, smaller);
  const density = (Math.max(300, (larger + smaller) * 70) * (end - start)) / 0.5;
  const count = Math.max(2, Math.ceil(((to - from) / (end - start)) * density));
  let path = "";
  for (let index = 0; index < count; index++) {
    const [across, down] = point(larger, smaller, from + ((to - from) * index) / (count - 1));
    path += (index ? "L" : "M") + across.toFixed(2) + " " + down.toFixed(2);
  }
  return path;
}

/** The whole curve, drawn once. */
export function fullCurve([larger, smaller]: Ratio): string {
  const [start, end] = spanFor(larger, smaller);
  return curvePath(larger, smaller, start, end);
}

/** A stroke that thins as the curve gets denser, so busy intervals do not turn into a block. */
export function strokeFor([larger, smaller]: Ratio, base: number): string {
  return (base * Math.min(1, Math.sqrt(7 / (larger + smaller)))).toFixed(2);
}

export function ratioOf(text: string | undefined): Ratio {
  const [larger = 3, smaller = 2] = (text ?? "3:2").split(":").map(Number);
  return [larger, smaller];
}
