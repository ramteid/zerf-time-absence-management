import { writable } from "svelte/store";

export const currentUser = writable(null);

const THEME_KEY = "kitazeit.theme";

function readStoredTheme() {
  try {
    return localStorage.getItem(THEME_KEY) || "light";
  } catch {
    return "light";
  }
}

function applyTheme(t) {
  if (typeof document !== "undefined") {
    document.documentElement.setAttribute("data-theme", t);
  }
}

function createThemeStore() {
  const initial = readStoredTheme();
  applyTheme(initial);
  const { subscribe, update } = writable(initial);
  return {
    subscribe,
    toggle() {
      update((current) => {
        const next = current === "dark" ? "light" : "dark";
        try {
          localStorage.setItem(THEME_KEY, next);
        } catch {}
        applyTheme(next);
        return next;
      });
    },
  };
}

export const theme = createThemeStore();
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
