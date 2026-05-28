import { writable } from "svelte/store";

const THEME_KEY = "zerf.theme";

function readStoredTheme() {
  try {
    return localStorage.getItem(THEME_KEY) || "light";
  } catch {
    return "light";
  }
}

const THEME_COLORS = {
  light: "#f0eee9",
  dark: "#14161a",
};

function applyTheme(t) {
  if (typeof document !== "undefined") {
    document.documentElement.setAttribute("data-theme", t);
    const meta = document.querySelector('meta[name="theme-color"]');
    if (meta) meta.setAttribute("content", THEME_COLORS[t] ?? THEME_COLORS.light);
  }
}

function createThemeStore() {
  const initial = readStoredTheme();
  applyTheme(initial);
  const { subscribe, set: setStore } = writable(initial);
  return {
    subscribe,
    set(value) {
      try {
        localStorage.setItem(THEME_KEY, value);
      } catch {}
      applyTheme(value);
      setStore(value);
    },
  };
}

export const theme = createThemeStore();
