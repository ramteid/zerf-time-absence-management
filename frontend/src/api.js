// Thin API client. Business rules live on the backend; this client just
// renders forms, navigates, and submits HTTP calls.
import { writable } from "svelte/store";

const API = "/api/v1";

export const csrfToken = writable(null);
let _csrf = null;
csrfToken.subscribe((v) => (_csrf = v));

export async function api(path, opts = {}) {
  const headers = opts.body ? { "Content-Type": "application/json" } : {};
  const method = (opts.method || "GET").toUpperCase();
  if (_csrf && !["GET", "HEAD", "OPTIONS"].includes(method)) {
    headers["X-CSRF-Token"] = _csrf;
  }
  const r = await fetch(API + path, {
    headers,
    cache: ["GET", "HEAD", "OPTIONS"].includes(method)
      ? "no-store"
      : undefined,
    credentials: "same-origin",
    ...opts,
    body: opts.body ? JSON.stringify(opts.body) : undefined,
  });
  if (r.status === 204) return null;
  const ct = r.headers.get("content-type") || "";
  if (!ct.includes("json")) {
    if (!r.ok) throw new Error((await r.text()) || "Error");
    return r;
  }
  const d = await r.json();
  if (!r.ok) throw new Error(d.error || "Error");
  return d;
}
