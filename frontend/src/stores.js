import { writable } from "svelte/store";

export const currentUser = writable(null);
export const categories = writable([]);
export const settings = writable({ ui_language: "en" });
export const path = writable(
  typeof location !== "undefined" ? location.pathname + location.search : "/",
);

export function go(href, push = true) {
  if (typeof history === "undefined") return;
  if (push) history.pushState({}, "", href);
  else history.replaceState({}, "", href);
  path.set(location.pathname + location.search);
}

if (typeof window !== "undefined") {
  window.addEventListener("popstate", () => {
    path.set(location.pathname + location.search);
  });
}

export const toasts = writable([]);
let _id = 0;
export function toast(message, type = "info") {
  const id = ++_id;
  toasts.update((arr) => [...arr, { id, message, type }]);
  setTimeout(
    () => toasts.update((arr) => arr.filter((t) => t.id !== id)),
    3500,
  );
}
