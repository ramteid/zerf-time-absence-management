// Thin API client. Business rules live on the backend; this client just
// renders forms, navigates, and submits HTTP calls.
import { writable } from "svelte/store";

const API = "/api/v1";

export const csrfToken = writable(null);
let _csrf = null;
csrfToken.subscribe((v) => (_csrf = v));

// ── Session-expiry handler ───────────────────────────────────────────────────
// App.svelte registers handleSessionExpired here.  We use a Promise-based
// "in-flight" gate so that if many concurrent requests all return 401 at once
// only the first one triggers the redirect; the rest resolve silently once
// the handler has finished (preventing duplicate toasts, double-redirects, etc.)
let _unauthorizedHandler = null;
let _handlingUnauthorized = false;
let _gateResetHandler = null;

export function setUnauthorizedHandler(fn) {
  _unauthorizedHandler = fn;
}

/**
 * Register a callback that fires whenever resetUnauthorizedGate() is called.
 * App.svelte uses this to also reset its own local _sessionExpiredHandling flag.
 */
export function setGateResetHandler(fn) {
  _gateResetHandler = fn;
}

/** Reset the gate (called by Login.svelte after a successful login). */
export function resetUnauthorizedGate() {
  _handlingUnauthorized = false;
  if (_gateResetHandler) _gateResetHandler();
}

function handleUnauthorized() {
  if (_handlingUnauthorized) return;
  _handlingUnauthorized = true;
  if (_unauthorizedHandler) {
    _unauthorizedHandler();
  }
}

// ── Main fetch wrapper ───────────────────────────────────────────────────────
export async function api(path, opts = {}) {
  const headers = opts.body ? { "Content-Type": "application/json" } : {};
  const method = (opts.method || "GET").toUpperCase();
  if (_csrf && !["GET", "HEAD", "OPTIONS"].includes(method)) {
    headers["X-CSRF-Token"] = _csrf;
  }

  let r;
  try {
    r = await fetch(API + path, {
      headers,
      cache: ["GET", "HEAD", "OPTIONS"].includes(method)
        ? "no-store"
        : undefined,
      credentials: "same-origin",
      ...opts,
      body: opts.body ? JSON.stringify(opts.body) : undefined,
    });
  } catch {
    // Distinguish a real network failure (offline, DNS, etc.) from an auth
    // failure. Re-throw as a typed error so callers can handle it differently
    // from business-logic errors. Does NOT trigger the session-expiry handler.
    const e = new Error("Network error. Please check your connection.");
    e.isNetworkError = true;
    throw e;
  }

  if (r.status === 204) return null;

  // Session is gone or CSRF token is stale — force the user back to login.
  // Skip this for the auth endpoints themselves to avoid redirect loops.
  if ((r.status === 401 || r.status === 403) && !path.startsWith("/auth/")) {
    handleUnauthorized();
    throw new Error("Session expired. Please sign in again.");
  }

  const ct = r.headers.get("content-type") || "";
  if (!ct.includes("json")) {
    if (!r.ok) throw new Error((await r.text()) || "Error");
    return r;
  }
  const d = await r.json();
  if (!r.ok) throw new Error(d.error || "Error");
  return d;
}
