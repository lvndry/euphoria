// @ts-check

/** @typedef {[number, number]} Ratio Two whole numbers of an interval, larger first, such as [3, 2] for a fifth. */
/** @typedef {{ number: number, letter: string }} CamelotKey A Camelot code: 1 to 12, and A for minor or B for major. */

(function () {
  document.documentElement.classList.add("js");
  const DRAW = 1.6, HOLD = 0.5, ERASE = 1.2, LOOP = DRAW + HOLD + ERASE, PULSE = 2.4, DEPTH = 0.55;
  const reduces = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  /**
   * @param {string} id
   * @returns {HTMLElement}
   */
  function required(id) {
    const element = document.getElementById(id);
    if (!element) throw new Error(`#${id} is missing from the page`);
    return element;
  }

  /** @param {number} progress */
  const ease = (progress) => 0.5 - 0.5 * Math.cos(Math.PI * Math.min(Math.max(progress, 0), 1));

  /**
   * @param {number} larger
   * @param {number} smaller
   * @param {number} turn
   * @returns {[number, number]}
   */
  const point = (larger, smaller, turn) => [32 + 24 * Math.sin(larger * 2 * Math.PI * turn), 32 + 24 * Math.cos(smaller * 2 * Math.PI * turn)];

  /** @type {Record<string, [number, number]>} */
  const spanCache = {};
  /**
   * The stretch of the curve's turn that draws it once: half a turn when the second half retraces the first.
   * @param {number} larger
   * @param {number} smaller
   * @returns {[number, number]}
   */
  function spanFor(larger, smaller) {
    const key = larger + ":" + smaller;
    const cached = spanCache[key];
    if (cached) return cached;
    let retraces = true;
    for (let step = 0; step <= 200 && retraces; step++) {
      const turn = 0.25 + 0.5 * step / 200;
      const [ax, ay] = point(larger, smaller, turn);
      const [bx, by] = point(larger, smaller, 1.5 - turn);
      if (Math.abs(ax - bx) > 1e-3 || Math.abs(ay - by) > 1e-3) retraces = false;
    }
    return (spanCache[key] = retraces ? [0.25, 0.75] : [0.25, 1.25]);
  }

  /**
   * @param {number} larger
   * @param {number} smaller
   * @param {number} from
   * @param {number} to
   */
  function curvePath(larger, smaller, from, to) {
    if (to - from <= 0) return "";
    const [start, end] = spanFor(larger, smaller);
    const density = Math.max(300, (larger + smaller) * 70) * (end - start) / 0.5;
    const count = Math.max(2, Math.ceil((to - from) / (end - start) * density));
    let path = "";
    for (let index = 0; index < count; index++) {
      const [across, down] = point(larger, smaller, from + (to - from) * index / (count - 1));
      path += (index ? "L" : "M") + across.toFixed(2) + " " + down.toFixed(2);
    }
    return path;
  }

  /**
   * The part of the curve on show `seconds` in: drawn, held, then erased, as the app's logo does.
   * @param {number} seconds
   * @param {number} start
   * @param {number} end
   * @param {boolean} loops
   * @returns {[number, number]}
   */
  function span(seconds, start, end, loops) {
    const length = end - start;
    const local = loops ? ((seconds % LOOP) + LOOP) % LOOP : Math.min(seconds, DRAW + HOLD);
    if (local < DRAW) return [start, start + length * ease(local / DRAW)];
    if (local < DRAW + HOLD || !loops) return [start, end];
    return [start + length * ease((local - DRAW - HOLD) / ERASE), end];
  }

  /**
   * @param {string | undefined} text
   * @returns {Ratio}
   */
  function ratioOf(text) {
    const [larger = 3, smaller = 2] = (text ?? "3:2").split(":").map(Number);
    return [larger, smaller];
  }

  const fullMark = curvePath(3, 2, 0.25, 0.75);
  document.querySelectorAll("path.full").forEach((path) => path.setAttribute("d", fullMark));

  /** @param {number} number */
  const hue = (number) => (number * 30) % 360;
  /**
   * @param {number} larger
   * @param {number} smaller
   * @param {number} base
   */
  const strokeFor = (larger, smaller, base) => (base * Math.min(1, Math.sqrt(7 / (larger + smaller)))).toFixed(2);

  /** @type {Array<[string, string, string, number]>} */
  const intervals = [
    ["1:1", "Same key", "8A to 8A", 240],
    ["3:2", "Perfect fifth", "8A to 9A", 270],
    ["4:3", "Perfect fourth", "8A to 7A", 210],
    ["6:5", "Relative major", "8A to 8B", 240],
    ["9:8", "Energy boost", "8A to 10A", 300],
    ["45:32", "A clash", "8A to 2A", 60],
  ];
  const intervalsBox = document.getElementById("intervals");
  if (intervalsBox) intervalsBox.innerHTML = intervals.map(([ratio, name, move, keyHue], index) => {
    const [larger, smaller] = ratioOf(ratio);
    return `<div class="interval"><svg viewBox="0 0 64 64" aria-hidden="true"><path class="mark-path" data-curve="${ratio}" data-loop data-offset="${(1.2 + index * 0.45).toFixed(2)}" stroke-width="${strokeFor(larger, smaller, 2.6)}" style="color:oklch(0.6 0.14 ${keyHue})"></path></svg><b>${name}</b><span>${move} · ${ratio.replace(":", " : ")}</span></div>`;
  }).join("");

  /** @type {WeakMap<SVGPathElement, string>} */
  const drawn = new WeakMap();
  /**
   * Draws the stretch of a curve, rebuilding the path only when what is on screen changes.
   * @param {SVGPathElement} path
   * @param {number} larger
   * @param {number} smaller
   * @param {number} from
   * @param {number} to
   */
  function draw(path, larger, smaller, from, to) {
    const key = `${larger}:${smaller}:${from.toFixed(4)}:${to.toFixed(4)}`;
    if (drawn.get(path) === key) return;
    drawn.set(path, key);
    path.setAttribute("d", curvePath(larger, smaller, from, to));
  }

  const tracked = new Set(/** @type {NodeListOf<SVGPathElement>} */ (document.querySelectorAll("path[data-curve]")));
  /** @type {Map<SVGPathElement, number>} */
  const started = new Map();
  /** @param {SVGPathElement} path */
  const restart = (path) => started.set(path, performance.now());
  tracked.forEach((path) => { if (!path.hasAttribute("data-on-view")) restart(path); });

  /** @param {number} now */
  function frame(now) {
    tracked.forEach((path) => {
      const [larger, smaller] = ratioOf(path.dataset.curve);
      const [start, end] = spanFor(larger, smaller);
      const startedAt = started.get(path);
      if (startedAt === undefined) {
        draw(path, larger, smaller, start, end);
        return;
      }
      const seconds = (now - startedAt) / 1000 - Number(path.dataset.offset || 0);
      if (reduces) {
        draw(path, larger, smaller, start, end);
        path.style.strokeOpacity = String(1 - DEPTH * (0.5 - 0.5 * Math.cos(2 * Math.PI * Math.max(seconds, 0) / PULSE)));
        return;
      }
      if (seconds < 0) { draw(path, larger, smaller, start, start); return; }
      const [from, to] = span(seconds, start, end, path.hasAttribute("data-loop"));
      draw(path, larger, smaller, from, to);
    });
    beatFrame(now);
    requestAnimationFrame(frame);
  }

  const heroMark = document.getElementById("hero-mark");
  const heroPath = heroMark ? heroMark.querySelector("path") : null;
  if (heroMark && heroPath) heroMark.addEventListener("click", () => restart(heroPath));
  const closingMark = document.getElementById("closing-mark");
  const closingPath = closingMark ? closingMark.querySelector("path") : null;
  if (closingMark && closingPath) closingMark.addEventListener("click", () => restart(closingPath));
  const brandPath = /** @type {SVGPathElement} */ (/** @type {unknown} */ (required("brand-path")));
  required("brand").addEventListener("mouseenter", () => {
    if (reduces || brandPath.dataset.curve) return;
    brandPath.dataset.curve = "3:2";
    tracked.add(brandPath);
    restart(brandPath);
    setTimeout(() => { tracked.delete(brandPath); drawn.delete(brandPath); delete brandPath.dataset.curve; brandPath.setAttribute("d", fullMark); }, (DRAW + HOLD) * 1000);
  });

  const beats = document.getElementById("beats") || document.createElement("div");
  beats.innerHTML = Array.from({ length: 16 }, (_, index) => `<i class="${index % 4 === 0 ? "bar" : ""}"></i>`).join("");
  const beatCells = Array.from(beats.children);
  const beatSeconds = 60 / 117.9;
  let lastBeat = -1;
  /** @param {number} now */
  function beatFrame(now) {
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
  /** @type {Ratio[]} */
  const RATIOS = [[1, 1], [16, 15], [9, 8], [6, 5], [5, 4], [4, 3], [45, 32], [3, 2], [8, 5], [5, 3], [9, 5], [15, 8]];
  const INTERVAL_NAMES = ["unison", "minor second", "major second", "minor third", "major third", "perfect fourth", "tritone", "perfect fifth", "minor sixth", "major sixth", "minor seventh", "major seventh"];
  /** @param {CamelotKey} key */
  const keyName = (key) => (key.letter === "A" ? MINOR_NAMES[key.number - 1] + " minor" : MAJOR_NAMES[key.number - 1] + " major");
  /** @param {CamelotKey} key */
  const pitchOf = (key) => (key.letter === "A" ? MINOR_PITCH : MAJOR_PITCH)[key.number - 1] ?? 0;
  /** @param {number} semitones */
  const ratioFor = (semitones) => RATIOS[semitones] ?? RATIOS[0] ?? [1, 1];
  /** @param {number} number */
  const wrap = (number) => ((number - 1 + 120) % 12) + 1;

  const movesBox = document.getElementById("moves");
  if (movesBox) {
    /** @type {Array<[string, string, number, string, string]>} */
    const MOVES = [
      ["Same key", "8A to 8A", 240, "mixes", "The same key. Always safe."],
      ["One step on the wheel", "8A to 9A", 270, "mixes", "A fifth apart. The classic harmonic mix."],
      ["One step on the wheel", "8A to 7A", 210, "mixes", "A fifth the other way: a fourth up."],
      ["Relative", "8A to 8B", 240, "mixes", "The other letter, the same number: the same notes in major."],
      ["Wheel diagonal", "8A to 7B", 210, "bold", "One step and the other letter. Six of seven notes are shared, so the mood shifts while the blend holds."],
      ["Energy boost", "8A to 10A", 300, "bold", "Two numbers up. The key rises by a whole tone, a clear lift."],
      ["Mood change", "8A to 11B", 330, "bold", "The same home note in the other mode, A minor to A major. A dramatic change of mood."],
      ["Semitone boost", "8A to 3A", 90, "bold", "Seven numbers up. The key rises by a semitone, a dramatic lift whose melodies clash."],
      ["A clash", "8A to 2A", 60, "clash", "Keys a tritone apart share little, so Euphoria never suggests this."],
    ];
    /**
     * @param {string} code
     * @returns {CamelotKey}
     */
    const parse = (code) => ({ number: parseInt(code, 10), letter: code.slice(-1) });
    movesBox.innerHTML = MOVES.map(([name, fromTo, keyHue, kind, description], index) => {
      const [fromCode = "8A", toCode = "8A"] = fromTo.split(" to ");
      const semitones = (pitchOf(parse(toCode)) - pitchOf(parse(fromCode)) + 12) % 12;
      const [larger, smaller] = ratioFor(semitones);
      const label = kind === "mixes" ? "Mixes in key" : kind === "bold" ? "Bolder move" : "Clashes";
      return `<div class="move"><svg viewBox="0 0 64 64" aria-hidden="true"><path class="mark-path" data-curve="${larger}:${smaller}" data-loop data-offset="${(index * 0.35).toFixed(2)}" stroke-width="${strokeFor(larger, smaller, 2.4)}" style="color:oklch(0.58 0.15 ${keyHue})"></path></svg><b>${name}</b><span class="from">${fromTo} · ${larger} : ${smaller}</span><p>${description}</p><span class="tag ${kind}">${label}</span></div>`;
    }).join("");
    /** @type {NodeListOf<SVGPathElement>} */ (movesBox.querySelectorAll("path[data-curve]")).forEach((path) => { tracked.add(path); restart(path); });
  }

  if (document.getElementById("this-keys")) {
    const MIXES = new Set(["Same key", "One step on the wheel", "Relative"]);
    /**
     * The name the app gives the move from one key to the next, or "" when it clashes.
     * @param {CamelotKey} current
     * @param {CamelotKey} next
     */
    function moveName(current, next) {
      const step = ((next.number - current.number) % 12 + 12) % 12;
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
    /** @type {{ current: CamelotKey, next: CamelotKey }} */
    const state = { current: { number: 8, letter: "A" }, next: { number: 9, letter: "A" } };

    /**
     * @param {Event} event
     * @returns {HTMLButtonElement | null}
     */
    const clickedButton = (event) => (event.target instanceof Element ? event.target.closest("button") : null);

    /**
     * @param {string} containerId
     * @param {"current" | "next"} which
     */
    function buildKeys(containerId, which) {
      const container = required(containerId);
      container.innerHTML = Array.from({ length: 12 }, (_, index) => `<button type="button" style="--hue:${hue(index + 1)}" data-number="${index + 1}">${index + 1}</button>`).join("");
      container.addEventListener("click", (event) => {
        const button = clickedButton(event);
        if (!button) return;
        state[which].number = Number(button.dataset.number);
        update();
      });
    }
    /**
     * @param {string} containerId
     * @param {"current" | "next"} which
     */
    function buildMode(containerId, which) {
      required(containerId).addEventListener("click", (event) => {
        const button = clickedButton(event);
        if (!button) return;
        state[which].letter = button.dataset.letter ?? state[which].letter;
        update();
      });
    }
    buildKeys("this-keys", "current");
    buildKeys("next-keys", "next");
    buildMode("this-mode", "current");
    buildMode("next-mode", "next");

    const presets = document.querySelector(".presets");
    if (presets) presets.addEventListener("click", (event) => {
      const button = clickedButton(event);
      if (!button) return;
      const move = button.dataset.move;
      const current = state.current;
      const other = current.letter === "A" ? "B" : "A";
      const minor = current.letter === "A";
      if (move === "relative") state.next = { number: current.number, letter: other };
      else if (move === "diagonal") state.next = { number: wrap(current.number + (minor ? -1 : 1)), letter: other };
      else if (move === "mood") state.next = { number: wrap(current.number + (minor ? 3 : -3)), letter: other };
      else state.next = { number: wrap(current.number + Number(move)), letter: current.letter };
      update();
    });

    const mixCurve = /** @type {SVGPathElement} */ (/** @type {unknown} */ (required("mix-curve")));
    const mixGhost = required("mix-ghost");
    function update() {
      /** @type {Array<"current" | "next">} */ (["current", "next"]).forEach((which) => {
        const key = state[which];
        const prefix = which === "current" ? "this" : "next";
        /** @type {NodeListOf<HTMLButtonElement>} */ (document.querySelectorAll(`#${prefix}-keys button`)).forEach((button) => {
          button.setAttribute("aria-pressed", String(Number(button.dataset.number) === key.number));
          button.textContent = (button.dataset.number ?? "") + key.letter;
        });
        /** @type {NodeListOf<HTMLButtonElement>} */ (document.querySelectorAll(`#${prefix}-mode button`)).forEach((button) => button.setAttribute("aria-pressed", String(button.dataset.letter === key.letter)));
        required(`${prefix}-name`).textContent = `${key.number}${key.letter} · ${keyName(key)}`;
      });
      const { current, next } = state;
      const semitones = (pitchOf(next) - pitchOf(current) + 12) % 12;
      const [larger, smaller] = ratioFor(semitones);
      const move = moveName(current, next);
      required("verdict").textContent = move ? (MIXES.has(move) ? "Mixes well." : "A bolder move.") : "Clashes.";
      const intervalName = INTERVAL_NAMES[semitones] ?? "";
      required("verdict-sub").textContent = `${move ? move + ". " : ""}${keyName(current)} to ${keyName(next)} is ${semitones === 0 ? "the same home note" : "a " + intervalName}, ${larger} : ${smaller}.`;
      mixCurve.dataset.curve = `${larger}:${smaller}`;
      mixCurve.setAttribute("stroke-width", strokeFor(larger, smaller, 1.6));
      mixCurve.style.color = `oklch(0.58 0.15 ${hue(next.number)})`;
      const [start, end] = spanFor(larger, smaller);
      mixGhost.setAttribute("d", curvePath(larger, smaller, start, end));
      mixGhost.setAttribute("stroke-width", strokeFor(larger, smaller, 1.6));
      restart(mixCurve);
    }
    update();
  }

  /** @type {NodeListOf<HTMLButtonElement>} */ (document.querySelectorAll("[data-theme-shot]")).forEach((button) => button.addEventListener("click", () => {
    const showPaper = button.dataset.themeShot === "paper";
    required("shot-paper").classList.toggle("is-hidden", !showPaper);
    required("shot-slate").classList.toggle("is-hidden", showPaper);
    document.querySelectorAll("[data-theme-shot]").forEach((other) => other.setAttribute("aria-pressed", String(other === button)));
  }));

  const tabs = Array.from(document.querySelectorAll(".term-tabs button"));
  tabs.forEach((tab) => tab.addEventListener("click", () => tabs.forEach((other) => {
    other.setAttribute("aria-selected", String(other === tab));
    required(other.getAttribute("aria-controls") ?? "").hidden = other !== tab;
  })));

  /** @type {NodeListOf<HTMLButtonElement>} */ (document.querySelectorAll(".copy")).forEach((button) => button.addEventListener("click", () => {
    const code = button.parentElement ? button.parentElement.querySelector("code") : null;
    if (!code) return;
    const reset = () => setTimeout(() => { button.textContent = "Copy"; }, 1600);
    const fallback = () => {
      const range = document.createRange();
      range.selectNodeContents(code);
      const selection = window.getSelection();
      if (selection) { selection.removeAllRanges(); selection.addRange(range); }
      let copied = false;
      try { copied = document.execCommand("copy"); } catch (error) { copied = false; }
      button.textContent = copied ? "Copied" : "Selected, press ⌘C";
      reset();
    };
    try { navigator.clipboard.writeText(code.textContent ?? "").then(() => { button.textContent = "Copied"; reset(); }, fallback); } catch (error) { fallback(); }
  }));

  if ("IntersectionObserver" in window && !reduces) {
    const rises = Array.from(document.querySelectorAll(".rise"));
    const viewport = window.innerHeight;
    rises.forEach((element) => { if (element.getBoundingClientRect().top > viewport) element.classList.add("pending"); });
    const observer = new IntersectionObserver((entries) => entries.forEach((entry) => {
      if (!entry.isIntersecting) return;
      entry.target.classList.remove("pending");
      observer.unobserve(entry.target);
    }), { threshold: 0 });
    rises.forEach((element) => observer.observe(element));
    const closingObserver = new IntersectionObserver((entries) => entries.forEach((entry) => {
      if (entry.isIntersecting && closingPath) { restart(closingPath); closingObserver.disconnect(); }
    }), { threshold: 0.4 });
    if (closingMark) closingObserver.observe(closingMark);
  } else if (closingPath) {
    restart(closingPath);
  }

  /** @type {Array<[string, string, string, Ratio, number]>} */
  const TOOLS = [
    ["Mixes well with", "Beside every track in the inspector: the tracks that mix after it, ranked by key move, tempo and energy.", "library", [3, 2], 240],
    ["Search by key", "Type 8A in the command palette for 8A first, then one step and relative, marked as pills.", "palette", [3, 1], 60],
    ["Next on deck B", "With a track on deck A, the decks list what mixes well after it. Choose which moves count.", "decks", [1, 1], 180],
    ["Plan a transition", "Pick the next track in the focus view and get the window, the bars and the key relation in a sentence.", "focus", [4, 3], 150],
    ["Sets in key", "The set builder orders tracks along an energy curve with every mix in a compatible key.", "set", [5, 3], 270],
    ["The key path of a playlist", "Above a playlist, each key in play order, with a red mark on any step that does not mix.", "set", [5, 4], 330],
  ];
  const toolsList = document.getElementById("tools-list");
  if (toolsList) toolsList.innerHTML = TOOLS.map(([name, description, id, [larger, smaller], toolHue]) => `<a class="tool" href="features.html#${id}"><span class="icon"><svg viewBox="0 0 64 64" aria-hidden="true"><path class="mark-path" stroke-width="3.4" d="${curvePath(larger, smaller, 0, 1)}" style="color:oklch(0.58 0.15 ${toolHue})"></path></svg></span><span><b>${name}</b><span>${description}</span></span></a>`).join("");

  requestAnimationFrame(frame);
})();
