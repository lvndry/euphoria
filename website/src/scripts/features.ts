// The features page: the rail of areas becomes tabs, one panel shown at a time, each with its own address.

function required(id: string): HTMLElement {
  const element = document.getElementById(id);
  if (!element) throw new Error(`#${id} is missing from the page`);
  return element;
}

const narrow = (): boolean => window.matchMedia("(max-width: 980px)").matches;
const rail = required("rail");
const panels = required("panels");
const tabs = Array.from(rail.querySelectorAll<HTMLElement>(".tab"));
const ids = tabs.map((tab) => tab.id.replace(/^tab-/, ""));

/**
 * Shows one feature's panel and hides the others, and on a phone brings its chip into view. A tab the
 * visitor picks puts its address in the URL; the first tab shown on arrival does not, so the page opens
 * at its top rather than jumping to the panel.
 */
function show(id: string, scroll: boolean, remember = true): void {
  const chosen = ids.includes(id) ? id : ids[0];
  if (chosen === undefined) return;
  for (const candidate of ids) {
    const selected = candidate === chosen;
    required(`tab-${candidate}`).setAttribute("aria-selected", String(selected));
    required(candidate).hidden = !selected;
  }
  if (narrow()) required(`tab-${chosen}`).scrollIntoView({ inline: "center", block: "nearest" });
  if (remember && location.hash.slice(1) !== chosen) history.replaceState(null, "", `#${chosen}`);
  if (scroll) panels.scrollIntoView({ block: "start" });
}

rail.addEventListener("click", (event) => {
  const tab = event.target instanceof Element ? event.target.closest(".tab") : null;
  if (tab) show(tab.id.replace(/^tab-/, ""), narrow());
});
rail.addEventListener("keydown", (event) => {
  if (!["ArrowDown", "ArrowUp", "ArrowLeft", "ArrowRight"].includes(event.key)) return;
  event.preventDefault();
  const position = tabs.findIndex((tab) => tab === document.activeElement);
  const step = event.key === "ArrowDown" || event.key === "ArrowRight" ? 1 : -1;
  const target = tabs[(position + step + tabs.length) % tabs.length];
  if (!target) return;
  target.focus();
  show(target.id.replace(/^tab-/, ""), false);
});
panels.addEventListener("click", (event) => {
  const button = event.target instanceof Element ? event.target.closest("[data-go]") : null;
  if (button instanceof HTMLElement && button.dataset["go"]) show(button.dataset["go"], true);
});
window.addEventListener("hashchange", () => show(location.hash.slice(1), false));
show(location.hash.slice(1), false, false);
