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
  if (push) history.pushState({}, "", href);
  else history.replaceState({}, "", href);
  const after = location.pathname + location.search;
  console.debug("[nav-debug]", "go", { href, push, before, after });
  path.set(after);
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
