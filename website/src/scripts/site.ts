import { curvePath, fullCurve, ratioOf, spanFor, strokeFor, type Ratio } from "../lib/curves";

/** A Camelot code: 1 to 12, and A for minor or B for major. */
interface CamelotKey {
  number: number;
  letter: string;
}

document.documentElement.classList.add("js");

// The logo's timing in the app (apps/euphoria-desktop/src/logo_motion.rs).
const DRAW = 1.6;
const HOLD = 0.5;
const ERASE = 1.2;
const LOOP = DRAW + HOLD + ERASE;
const PULSE = 2.4;
const DEPTH = 0.55;
const reduces = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

function required<Found extends HTMLElement | SVGElement = HTMLElement>(id: string): Found {
  const element = document.getElementById(id);
  if (!element) throw new Error(`#${id} is missing from the page`);
  return element as unknown as Found;
}

const ease = (progress: number): number => 0.5 - 0.5 * Math.cos(Math.PI * Math.min(Math.max(progress, 0), 1));

/** The part of the curve on show `seconds` in: drawn, held, then erased, as the app's logo does. */
function span(seconds: number, start: number, end: number, loops: boolean): [number, number] {
  const length = end - start;
  const local = loops ? ((seconds % LOOP) + LOOP) % LOOP : Math.min(seconds, DRAW + HOLD);
  if (local < DRAW) return [start, start + length * ease(local / DRAW)];
  if (local < DRAW + HOLD || !loops) return [start, end];
  return [start + length * ease((local - DRAW - HOLD) / ERASE), end];
}

const hue = (number: number): number => (number * 30) % 360;
const fullMark = fullCurve([3, 2]);
document.querySelectorAll("path.full").forEach((path) => path.setAttribute("d", fullMark));
document.querySelectorAll<SVGPathElement>("path[data-static]").forEach((path) => {
  path.setAttribute("d", fullCurve(ratioOf(path.dataset["curve"])));
});

const drawn = new WeakMap<SVGPathElement, string>();
/** Draws the stretch of a curve, rebuilding the path only when what is on screen changes. */
function draw(path: SVGPathElement, [larger, smaller]: Ratio, from: number, to: number): void {
  const key = `${larger}:${smaller}:${from.toFixed(4)}:${to.toFixed(4)}`;
  if (drawn.get(path) === key) return;
  drawn.set(path, key);
  path.setAttribute("d", curvePath(larger, smaller, from, to));
}

const tracked = new Set(document.querySelectorAll<SVGPathElement>("path[data-curve]:not([data-static])"));
const started = new Map<SVGPathElement, number>();
const restart = (path: SVGPathElement): void => {
  started.set(path, performance.now());
};
tracked.forEach((path) => {
  if (!path.hasAttribute("data-on-view")) restart(path);
});

function frame(now: number): void {
  tracked.forEach((path) => {
    const ratio = ratioOf(path.dataset["curve"]);
    const [start, end] = spanFor(...ratio);
    const startedAt = started.get(path);
    if (startedAt === undefined) {
      draw(path, ratio, start, end);
      return;
    }
    const seconds = (now - startedAt) / 1000 - Number(path.dataset["offset"] || 0);
    if (reduces) {
      draw(path, ratio, start, end);
      path.style.strokeOpacity = String(1 - DEPTH * (0.5 - 0.5 * Math.cos((2 * Math.PI * Math.max(seconds, 0)) / PULSE)));
      return;
    }
    if (seconds < 0) {
      draw(path, ratio, start, start);
      return;
    }
    const [from, to] = span(seconds, start, end, path.hasAttribute("data-loop"));
    draw(path, ratio, from, to);
  });
  beatFrame(now);
  requestAnimationFrame(frame);
}

const heroMark = document.getElementById("hero-mark");
const heroPath = heroMark?.querySelector("path") ?? null;
if (heroMark && heroPath) heroMark.addEventListener("click", () => restart(heroPath));
const closingMark = document.getElementById("closing-mark");
const closingPath = closingMark?.querySelector("path") ?? null;
if (closingMark && closingPath) closingMark.addEventListener("click", () => restart(closingPath));
const brandPath = required<SVGPathElement>("brand-path");
required("brand").addEventListener("mouseenter", () => {
  if (reduces || brandPath.dataset["curve"]) return;
  brandPath.dataset["curve"] = "3:2";
  tracked.add(brandPath);
  restart(brandPath);
  setTimeout(() => {
    tracked.delete(brandPath);
    drawn.delete(brandPath);
    delete brandPath.dataset["curve"];
    brandPath.setAttribute("d", fullMark);
  }, (DRAW + HOLD) * 1000);
});

const beats = document.getElementById("beats") ?? document.createElement("div");
beats.innerHTML = Array.from({ length: 16 }, (_, index) => `<i class="${index % 4 === 0 ? "bar" : ""}"></i>`).join("");
const beatCells = Array.from(beats.children);
const beatSeconds = 60 / 117.9;
let lastBeat = -1;
function beatFrame(now: number): void {
  if (reduces) return;
  const beat = Math.floor(now / 1000 / beatSeconds) % 16;
  if (beat === lastBeat) return;
  lastBeat = beat;
  beatCells.forEach((cell, index) => cell.classList.toggle("on", index === beat));
}

const MINOR_NAMES = ["Ab", "Eb", "Bb", "F", "C", "G", "D", "A", "E", "B", "F#", "Db"];
const MAJOR_NAMES = ["B", "F#", "Db", "Ab", "Eb", "Bb", "F", "C", "G", "D", "A", "E"];
const MINOR_PITCH = [8, 3, 10, 5, 0, 7, 2, 9, 4, 11, 6, 1];
const MAJOR_PITCH = [11, 6, 1, 8, 3, 10, 5, 0, 7, 2, 9, 4];
const RATIOS: Ratio[] = [[1, 1], [16, 15], [9, 8], [6, 5], [5, 4], [4, 3], [45, 32], [3, 2], [8, 5], [5, 3], [9, 5], [15, 8]];
const INTERVAL_NAMES = ["unison", "minor second", "major second", "minor third", "major third", "perfect fourth", "tritone", "perfect fifth", "minor sixth", "major sixth", "minor seventh", "major seventh"];
const keyName = (key: CamelotKey): string =>
  key.letter === "A" ? `${MINOR_NAMES[key.number - 1]} minor` : `${MAJOR_NAMES[key.number - 1]} major`;
const pitchOf = (key: CamelotKey): number => (key.letter === "A" ? MINOR_PITCH : MAJOR_PITCH)[key.number - 1] ?? 0;
const ratioFor = (semitones: number): Ratio => RATIOS[semitones] ?? [1, 1];
const wrap = (number: number): number => ((number - 1 + 120) % 12) + 1;
const MIXES = new Set(["Same key", "One step on the wheel", "Relative"]);

/** The name the app gives the move from one key to the next, or "" when it clashes. */
function moveName(current: CamelotKey, next: CamelotKey): string {
  const step = (((next.number - current.number) % 12) + 12) % 12;
  const minor = current.letter === "A";
  if (current.letter === next.letter) {
    if (step === 0) return "Same key";
    if (step === 1 || step === 11) return "One step on the wheel";
    if (step === 2) return "Energy boost";
    if (step === 7) return "Semitone boost";
    return "";
  }
  if (step === 0) return "Relative";
  if (step === (minor ? 11 : 1)) return "Wheel diagonal";
  if (step === (minor ? 3 : 9)) return "Mood change";
  return "";
}

function startMixer(): void {
  const state: { current: CamelotKey; next: CamelotKey } = { current: { number: 8, letter: "A" }, next: { number: 9, letter: "A" } };
  const clickedButton = (event: Event): HTMLButtonElement | null =>
    event.target instanceof Element ? event.target.closest("button") : null;
  const mixCurve = required<SVGPathElement>("mix-curve");
  const mixGhost = required<SVGPathElement>("mix-ghost");

  function update(): void {
    for (const which of ["current", "next"] as const) {
      const key = state[which];
      const prefix = which === "current" ? "this" : "next";
      document.querySelectorAll<HTMLButtonElement>(`#${prefix}-keys button`).forEach((button) => {
        button.setAttribute("aria-pressed", String(Number(button.dataset["number"]) === key.number));
        button.textContent = (button.dataset["number"] ?? "") + key.letter;
      });
      document.querySelectorAll<HTMLButtonElement>(`#${prefix}-mode button`).forEach((button) => {
        button.setAttribute("aria-pressed", String(button.dataset["letter"] === key.letter));
      });
      required(`${prefix}-name`).textContent = `${key.number}${key.letter} · ${keyName(key)}`;
    }
    const { current, next } = state;
    const semitones = (pitchOf(next) - pitchOf(current) + 12) % 12;
    const ratio = ratioFor(semitones);
    const move = moveName(current, next);
    required("verdict").textContent = move ? (MIXES.has(move) ? "Mixes well." : "A bolder move.") : "Clashes.";
    const interval = semitones === 0 ? "the same home note" : `a ${INTERVAL_NAMES[semitones] ?? ""}`;
    required("verdict-sub").textContent = `${move ? `${move}. ` : ""}${keyName(current)} to ${keyName(next)} is ${interval}, ${ratio[0]} : ${ratio[1]}.`;
    mixCurve.dataset["curve"] = ratio.join(":");
    mixCurve.setAttribute("stroke-width", strokeFor(ratio, 1.6));
    mixCurve.style.color = `oklch(0.58 0.15 ${hue(next.number)})`;
    mixGhost.setAttribute("d", fullCurve(ratio));
    mixGhost.setAttribute("stroke-width", strokeFor(ratio, 1.6));
    restart(mixCurve);
  }

  for (const [containerId, which] of [["this-keys", "current"], ["next-keys", "next"]] as const) {
    const container = required(containerId);
    container.innerHTML = Array.from({ length: 12 }, (_, index) => `<button type="button" style="--hue:${hue(index + 1)}" data-number="${index + 1}">${index + 1}</button>`).join("");
    container.addEventListener("click", (event) => {
      const button = clickedButton(event);
      if (!button) return;
      state[which].number = Number(button.dataset["number"]);
      update();
    });
  }
  for (const [containerId, which] of [["this-mode", "current"], ["next-mode", "next"]] as const) {
    required(containerId).addEventListener("click", (event) => {
      const button = clickedButton(event);
      if (!button) return;
      state[which].letter = button.dataset["letter"] ?? state[which].letter;
      update();
    });
  }
  document.querySelector(".presets")?.addEventListener("click", (event) => {
    const button = clickedButton(event);
    if (!button) return;
    const move = button.dataset["move"];
    const current = state.current;
    const other = current.letter === "A" ? "B" : "A";
    const minor = current.letter === "A";
    if (move === "relative") state.next = { number: current.number, letter: other };
    else if (move === "diagonal") state.next = { number: wrap(current.number + (minor ? -1 : 1)), letter: other };
    else if (move === "mood") state.next = { number: wrap(current.number + (minor ? 3 : -3)), letter: other };
    else state.next = { number: wrap(current.number + Number(move)), letter: current.letter };
    update();
  });
  update();
}
if (document.getElementById("this-keys")) startMixer();

document.querySelectorAll<HTMLButtonElement>("[data-theme-shot]").forEach((button) =>
  button.addEventListener("click", () => {
    const showPaper = button.dataset["themeShot"] === "paper";
    required("shot-paper").classList.toggle("is-hidden", !showPaper);
    required("shot-slate").classList.toggle("is-hidden", showPaper);
    document.querySelectorAll("[data-theme-shot]").forEach((other) => other.setAttribute("aria-pressed", String(other === button)));
  }),
);

const installTabs = Array.from(document.querySelectorAll(".term-tabs button"));
installTabs.forEach((tab) =>
  tab.addEventListener("click", () =>
    installTabs.forEach((other) => {
      other.setAttribute("aria-selected", String(other === tab));
      required(other.getAttribute("aria-controls") ?? "").hidden = other !== tab;
    }),
  ),
);

document.querySelectorAll<HTMLButtonElement>(".copy").forEach((button) =>
  button.addEventListener("click", () => {
    const code = button.parentElement?.querySelector("code");
    if (!code) return;
    const reset = (): void => {
      setTimeout(() => {
        button.textContent = "Copy";
      }, 1600);
    };
    const fallback = (): void => {
      const range = document.createRange();
      range.selectNodeContents(code);
      const selection = window.getSelection();
      selection?.removeAllRanges();
      selection?.addRange(range);
      let copied = false;
      try {
        copied = document.execCommand("copy");
      } catch {
        copied = false;
      }
      button.textContent = copied ? "Copied" : "Selected, press ⌘C";
      reset();
    };
    try {
      navigator.clipboard.writeText(code.textContent ?? "").then(() => {
        button.textContent = "Copied";
        reset();
      }, fallback);
    } catch {
      fallback();
    }
  }),
);

if ("IntersectionObserver" in window && !reduces) {
  const rises = Array.from(document.querySelectorAll(".rise"));
  const viewport = window.innerHeight;
  rises.forEach((element) => {
    if (element.getBoundingClientRect().top > viewport) element.classList.add("pending");
  });
  const observer = new IntersectionObserver(
    (entries) =>
      entries.forEach((entry) => {
        if (!entry.isIntersecting) return;
        entry.target.classList.remove("pending");
        observer.unobserve(entry.target);
      }),
    { threshold: 0 },
  );
  rises.forEach((element) => observer.observe(element));
  const closingObserver = new IntersectionObserver(
    (entries) =>
      entries.forEach((entry) => {
        if (entry.isIntersecting && closingPath) {
          restart(closingPath);
          closingObserver.disconnect();
        }
      }),
    { threshold: 0.4 },
  );
  if (closingMark) closingObserver.observe(closingMark);
} else if (closingPath) {
  restart(closingPath);
}

requestAnimationFrame(frame);
