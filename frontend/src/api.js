// Thin API client. Business rules live on the backend; this client just
// renders forms, navigates, and submits HTTP calls.
import { writable } from "svelte/store";
import { localizeErrorMessage } from "./i18n.js";

const API = "/api/v1";

export const csrfToken = writable(null);
let _csrf = null;
csrfToken.subscribe((csrfValue) => (_csrf = csrfValue));

// Session-expiry handler
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

function apiError(message) {
  const rawMessage = message || "Error";
  const error = new Error(localizeErrorMessage(rawMessage));
  error.apiMessage = rawMessage;
  return error;
}

// Main fetch wrapper
export async function api(path, opts = {}) {
  const method = (opts.method || "GET").toUpperCase();
  const isReadMethod = ["GET", "HEAD", "OPTIONS"].includes(method);
  const hasBody = Object.prototype.hasOwnProperty.call(opts, "body");
  const headers = new Headers(opts.headers || {});
  if (hasBody && !headers.has("Content-Type")) {
    headers.set("Content-Type", "application/json");
  }
  if (_csrf && !["GET", "HEAD", "OPTIONS"].includes(method)) {
    headers.set("X-CSRF-Token", _csrf);
  }

  let response;
  try {
    response = await fetch(API + path, {
      ...opts,
      headers,
      cache: opts.cache ?? (isReadMethod ? "no-store" : undefined),
      credentials: opts.credentials ?? "same-origin",
      body: hasBody ? JSON.stringify(opts.body) : undefined,
    });
  } catch {
    // Distinguish a real network failure (offline, DNS, etc.) from an auth
    // failure. Re-throw as a typed error so callers can handle it differently
    // from business-logic errors. Does NOT trigger the session-expiry handler.
    const networkError = apiError(
      "Network error. Please check your connection.",
    );
    networkError.isNetworkError = true;
    throw networkError;
  }

  if (response.status === 204) return null;

  // Session is gone or CSRF token is stale — force the user back to login.
  // Skip this for the auth endpoints themselves to avoid redirect loops.
  const isAuthEndpoint = path.startsWith("/auth/");
  if (response.status === 401 && !isAuthEndpoint) {
    handleUnauthorized();
    throw apiError("Session expired. Please sign in again.");
  }

  const contentType = response.headers.get("content-type") || "";
  if (!contentType.includes("json")) {
    if (!response.ok) throw apiError((await response.text()) || "Error");
    return response;
  }
  const payload = await response.json();
  if (!response.ok) throw apiError(payload.error || "Error");
  return payload;
}
