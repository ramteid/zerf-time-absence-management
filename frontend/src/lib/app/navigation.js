import { writable } from "svelte/store";

export const path = writable(
  typeof location !== "undefined" ? location.pathname + location.search : "/",
);

export function go(href, push = true) {
  if (typeof history === "undefined") return;
  const before =
    typeof location !== "undefined"
      ? location.pathname + location.search
      : null;
  const beforeHash = typeof location !== "undefined" ? location.hash : "";
  if (push) history.pushState({}, "", href);
  else history.replaceState({}, "", href);
  const after = location.pathname + location.search;
  const afterHash = typeof location !== "undefined" ? location.hash : "";
  // Only log navigation in debug builds; stripped from production bundles.
  if (__ZERF_DEBUG__) {
    console.debug("[nav-debug]", "go", { href, push, before, after });
  }
  path.set(after);
  // `history.pushState()` deliberately does not emit `hashchange`. Components
  // that react to in-page report anchors still need to observe an SPA
  // navigation whose pathname and query are unchanged but whose fragment moved.
  if (beforeHash !== afterHash && typeof window !== "undefined") {
    window.dispatchEvent(new Event("hashchange"));
  }
}

if (typeof window !== "undefined") {
  window.addEventListener("popstate", () => {
    const openDialogs = document.querySelectorAll("dialog[open]");
    if (openDialogs.length > 0) {
      openDialogs[openDialogs.length - 1].close();
      history.pushState({}, "", location.href);
      return;
    }
    path.set(location.pathname + location.search);
  });
}
