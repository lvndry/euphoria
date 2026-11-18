// @ts-check

(function () {
  /**
   * @param {string} id
   * @returns {HTMLElement}
   */
  function required(id) {
    const element = document.getElementById(id);
    if (!element) throw new Error(`#${id} is missing from the page`);
    return element;
  }
  /**
   * @param {number} larger
   * @param {number} smaller
   * @param {number} turn
   * @returns {[number, number]}
   */
  const point = (larger, smaller, turn) => [32 + 24 * Math.sin(larger * 2 * Math.PI * turn), 32 + 24 * Math.cos(smaller * 2 * Math.PI * turn)];
  /**
   * The stretch of the curve's turn that draws it once: half a turn when the second half retraces the first.
   * @param {number} larger
   * @param {number} smaller
   * @returns {[number, number]}
   */
  function spanFor(larger, smaller) {
    for (let step = 0; step <= 200; step++) {
      const turn = 0.25 + 0.5 * step / 200;
      const [ax, ay] = point(larger, smaller, turn);
      const [bx, by] = point(larger, smaller, 1.5 - turn);
      if (Math.abs(ax - bx) > 1e-3 || Math.abs(ay - by) > 1e-3) return [0.25, 1.25];
    }
    return [0.25, 0.75];
  }
  /** @param {Ratio} ratio */
  function fullCurve([larger, smaller]) {
    const [start, end] = spanFor(larger, smaller);
    const count = Math.ceil(Math.max(300, (larger + smaller) * 70) * (end - start) / 0.5);
    let path = "";
    for (let index = 0; index < count; index++) {
      const [across, down] = point(larger, smaller, start + (end - start) * index / (count - 1));
      path += (index ? "L" : "M") + across.toFixed(2) + " " + down.toFixed(2);
    }
    return path;
  }
  /**
   * @param {string | undefined} text
   * @returns {Ratio}
   */
  function ratioOf(text) {
    const [larger = 3, smaller = 2] = (text ?? "3:2").split(":").map(Number);
    return [larger, smaller];
  }

  document.querySelectorAll("path.full").forEach((path) => path.setAttribute("d", fullCurve([3, 2])));
  /** @type {NodeListOf<SVGPathElement>} */ (document.querySelectorAll("path[data-static]")).forEach((path) => path.setAttribute("d", fullCurve(ratioOf(path.dataset.curve))));

  const narrow = () => window.matchMedia("(max-width: 980px)").matches;
  const rail = required("rail");
  const panels = required("panels");
  const tabs = /** @type {HTMLElement[]} */ (Array.from(rail.querySelectorAll(".tab")));
  const ids = tabs.map((tab) => tab.id.replace(/^tab-/, ""));

  /**
   * Shows one feature's panel and hides the others, keeps its address in the URL, and on a phone brings its chip and panel into view.
   * @param {string} id
   * @param {boolean} scroll
   */
  function show(id, scroll) {
    const chosen = ids.includes(id) ? id : ids[0];
    if (chosen === undefined) return;
    ids.forEach((candidate) => {
      const selected = candidate === chosen;
      required(`tab-${candidate}`).setAttribute("aria-selected", String(selected));
      required(`panel-${candidate}`).hidden = !selected;
    });
    if (narrow()) required(`tab-${chosen}`).scrollIntoView({ inline: "center", block: "nearest" });
    if (location.hash.slice(1) !== chosen) history.replaceState(null, "", `#${chosen}`);
    if (scroll) panels.scrollIntoView({ block: "start" });
  }
  rail.addEventListener("click", (event) => {
    const tab = event.target instanceof Element ? event.target.closest(".tab") : null;
    if (tab) show(tab.id.replace(/^tab-/, ""), narrow());
  });
  rail.addEventListener("keydown", (event) => {
    if (!["ArrowDown", "ArrowUp", "ArrowLeft", "ArrowRight"].includes(event.key)) return;
    event.preventDefault();
    const index = tabs.findIndex((tab) => tab === document.activeElement);
    const step = event.key === "ArrowDown" || event.key === "ArrowRight" ? 1 : -1;
    const target = tabs[(index + step + tabs.length) % tabs.length];
    if (!target) return;
    target.focus();
    show(target.id.replace(/^tab-/, ""), false);
  });
  panels.addEventListener("click", (event) => {
    const button = event.target instanceof Element ? event.target.closest("[data-go]") : null;
    if (button instanceof HTMLElement && button.dataset.go) show(button.dataset.go, true);
  });
  window.addEventListener("hashchange", () => show(location.hash.slice(1), false));
  show(location.hash.slice(1), false);
})();
