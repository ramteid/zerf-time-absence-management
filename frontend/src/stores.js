import { writable } from "svelte/store";

export const currentUser = writable(null);

const THEME_KEY = "zerf.theme";

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
  const { subscribe, set: _set } = writable(initial);
  return {
    subscribe,
    set(value) {
      try {
        localStorage.setItem(THEME_KEY, value);
      } catch {}
      applyTheme(value);
      _set(value);
    },
  };
}

export const theme = createThemeStore();
export const categories = writable([]);
export const settings = writable({ ui_language: "en", time_format: "24h" });
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
    // Mobile back-button support: close the topmost open dialog instead of
    // navigating away. Use the last match (topmost in z-order) to handle
    // nested dialogs correctly (e.g., Confirm on top of EntryDialog).
    const openDialogs = document.querySelectorAll("dialog[open]");
    if (openDialogs.length > 0) {
      openDialogs[openDialogs.length - 1].close();
      history.pushState({}, "", location.href);
      return;
    }
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

// In-app notification center.
export const notifications = writable([]);
export const notificationsUnread = writable(0);

// Uses BroadcastChannel so that a logout or session expiry in one tab
// immediately propagates to every other open tab of the same origin.
// Message shape: { type: 'session-expired' | 'logout' }
let _sessionChannel = null;
try {
  if (typeof BroadcastChannel !== "undefined") {
    _sessionChannel = new BroadcastChannel("zerf-session");
  }
} catch {}

export function broadcastSession(type) {
  try {
    _sessionChannel?.postMessage({ type });
  } catch {}
}

/**
 * Register a handler for cross-tab session messages.
 * Returns an unsubscribe function.
 */
export function onSessionBroadcast(fn) {
  if (!_sessionChannel) return () => {};
  const handler = (e) => fn(e.data);
  _sessionChannel.addEventListener("message", handler);
  return () => _sessionChannel.removeEventListener("message", handler);
}
