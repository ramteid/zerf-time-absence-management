// Shared building blocks for the e2e spec files.
//
// The suite is split into one numbered spec file per scenario/role (see
// e2e/README.md for the full list and execution order). Anything that more
// than one spec file needs — signing in, completing the forced first-login
// password change, driving the app's custom date/time picker widgets, and a
// small on-disk store for credentials/sessions created in one file and
// consumed by another — lives here instead of being copy-pasted into each
// spec. When adding a new spec file, prefer extending this module over
// re-implementing one of these interactions locally.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { expect } from "@playwright/test";

// __dirname isn't available in ESM, so derive it from this module's own URL.
const __dirname = path.dirname(fileURLToPath(import.meta.url));
// Gitignored (see .gitignore) — holds per-role Playwright storageState JSON
// files and the shared credentials.json. Created fresh on every run by
// run.sh's ephemeral Docker stack, so nothing here needs to survive restarts.
const AUTH_DIR = path.join(__dirname, "..", ".auth");
fs.mkdirSync(AUTH_DIR, { recursive: true });

const CREDENTIALS_FILE = path.join(AUTH_DIR, "credentials.json");

// Path to the storageState file for a given role ("admin", "team_lead",
// "employee", "assistant"). A spec file that needs to act as an
// already-onboarded role resumes its session with:
//   test.use({ storageState: storageStatePath("employee") });
// instead of signing in again — Playwright restores the session cookie into
// a fresh browser context, and the app's own boot sequence (App.svelte calling
// GET /auth/me) re-populates the in-memory CSRF token, so no further setup is
// needed beyond a plain `page.goto("/")`.
export function storageStatePath(role) {
  return path.join(AUTH_DIR, `${role}.json`);
}

// Small on-disk store for credentials created by one spec file and consumed
// by another — most importantly, the temporary password an admin/team-lead
// reads off the "Temporary password:" dialog when creating a user, which a
// later spec file needs in order to perform that user's very first login
// (before any session/storageState exists for them yet).
//
// Each role's existing entry is overwritten once that user changes their own
// password (see changeTempPassword below), so the file always reflects the
// password actually in effect server-side. A plain JSON file is sufficient
// because the whole suite runs in a single Playwright worker — there is never
// concurrent access to this file from two specs at once.
export function readCredentials() {
  try {
    return JSON.parse(fs.readFileSync(CREDENTIALS_FILE, "utf8"));
  } catch {
    // First call of the run: file doesn't exist yet. Treat as empty.
    return {};
  }
}

export function writeCredential(role, email, password) {
  const all = readCredentials();
  all[role] = { email, password };
  fs.writeFileSync(CREDENTIALS_FILE, JSON.stringify(all, null, 2));
}

// Returns an ISO date string (YYYY-MM-DD) offset from "today" by `days`
// (negative = past, positive = future). Used throughout the suite to derive
// dates relative to whenever the suite happens to run, e.g.:
//   - a negative offset backdates a new user's contract start date so they
//     have bookable past weekdays (time entries can't be in the future, and
//     never before the user's start date);
//   - positive offsets push absence requests safely into the future so they
//     never collide with each other or with already-booked time entries.
// The offsets used by callers are large (weeks), so the exact wall-clock
// time and a one-day timezone skew between the test runner and the app's
// configured timezone never change which calendar day this lands on.
export function isoOffset(days) {
  const date = new Date();
  date.setDate(date.getDate() + days);
  return date.toISOString().slice(0, 10);
}

// Fills the Login form and submits it. Does not wait for the post-login
// redirect — callers should immediately assert on whatever the login lands
// on (the forced /account password-change screen for a temporary password,
// or the user's normal home route otherwise).
export async function signIn(page, email, password) {
  await page.goto("/");
  await page.locator("#email").fill(email);
  await page.locator("#password").fill(password);
  await page.getByRole("button", { name: "Sign in" }).click();
}

// Completes the forced first-login password change every newly created user
// hits on /account (the backend sets must_change_password=true whenever an
// admin/team-lead creates a user or resets/restores one — see AGENTS.md
// "Password reset: One-time 1h tokens, forced change on first login", which
// applies the same way to the temporary password issued at user creation).
//
// After the password is saved, this records the new password in
// credentials.json (so this same user's password is discoverable by any spec
// that might need to log in as them again from scratch) and snapshots the
// now-authenticated browser context as `storageState` under `role`, so later
// spec files can resume this exact session via
// `test.use({ storageState: storageStatePath(role) })` rather than repeating
// the sign-in + password-change dance.
export async function changeTempPassword(page, context, role, email, newPassword) {
  await page.waitForURL("**/account");
  await page.locator("#account-new-password").fill(newPassword);
  await page.locator("#account-confirm-password").fill(newPassword);
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByText("Password changed.")).toBeVisible();
  writeCredential(role, email, newPassword);
  await context.storageState({ path: storageStatePath(role) });
}

// Drives the app's flatpickr-backed DatePicker (src/DatePicker.svelte)
// directly through its own flatpickr instance, bypassing the calendar UI.
//
// Why not just click calendar days? flatpickr renders a popup calendar that
// has to be opened, then navigated month-by-month to reach the target date,
// then have the right day cell clicked — multiple steps, each a potential
// source of flakiness (the offset dates used across this suite can be months
// away from "today"). Driving the underlying flatpickr instance's setDate()
// API with triggerChange=true fires the exact same onChange callback a real
// click would, which is what actually updates the bound Svelte `value` — so
// this is equivalent to a real user interaction from the component's
// perspective, just without the brittle multi-step navigation.
//
// `altInputId` is the *visible* input's id (DatePicker.svelte assigns the id
// prop to flatpickr's altInput, not to the hidden original input it wraps).
// flatpickr keeps the actual instance on that hidden original input, which
// sits immediately before the altInput in the DOM, hence the
// previousElementSibling lookup.
export async function setDate(page, altInputId, iso) {
  // The DatePicker only builds its flatpickr instance in onMount, which
  // runs a tick after the surrounding route/dialog first renders. Callers
  // that invoke setDate as their very first interaction on a freshly
  // navigated page (no preceding `.fill()` to implicitly wait on) would
  // otherwise race that mount. Waiting for the element to attach makes this
  // safe regardless of what, if anything, ran before it.
  await page.locator(`#${altInputId}`).waitFor({ state: "attached" });
  await page.evaluate(
    ({ altInputId, iso }) => {
      const altInput = document.getElementById(altInputId);
      if (!altInput) throw new Error(`date input not found: ${altInputId}`);
      const original = altInput.previousElementSibling;
      const fp = (original && original._flatpickr) || altInput._flatpickr;
      if (!fp) throw new Error(`flatpickr instance not found: ${altInputId}`);
      fp.setDate(iso, true);
    },
    { altInputId, iso },
  );
  // Confirms the Svelte-bound value actually updated. In English locale (the
  // suite always switches the UI to English in 01-bootstrap.spec.js) the alt
  // display format for a plain date picker is the ISO string itself, so the
  // visible input's value should read back exactly what was set.
  await expect(page.locator(`#${altInputId}`)).toHaveValue(iso);
}

// Drives the app's custom TimePicker (src/TimePicker.svelte) — a button that
// opens a scrollable "drum" of hour/minute columns — via the keyboard
// digit-entry path the component implements for accessibility. Typing two
// digits for the hour and two for the minute commits that value immediately;
// minutes are snapped to 15-minute steps by the component itself, so callers
// must always pass a quarter-hour value (":00", ":15", ":30", ":45").
//
// The drum is closed via its own "OK" button rather than pressing Enter.
// This matters: the drum's keydown handler treats Enter as "close the drum",
// but Enter is not stopped from bubbling further up the DOM — and the
// EntryDialog/AbsenceDialog wrapping the time picker has its own Enter
// shortcut that submits ("saves") the whole dialog. Pressing Enter here would
// therefore close the *dialog* prematurely (with whatever was filled in so
// far), not just the time drum. The drum's "OK" button click handler stops
// propagation, so it closes only the drum.
export async function setTime(page, hiddenInputId, hhmm) {
  const root = `#${hiddenInputId} + .tp-root`;
  await page.locator(`${root} .tp-display`).click();
  // Give the drum a moment to open and move keyboard focus onto itself —
  // openPicker() in TimePicker.svelte focuses it via a setTimeout(…, 0), so a
  // synchronous click-then-type would race that focus move.
  await page.waitForTimeout(60);
  const [hours, minutes] = hhmm.split(":");
  for (const digit of hours) await page.keyboard.press(digit);
  for (const digit of minutes) await page.keyboard.press(digit);
  await page.locator(`${root} .tp-ok`).click();
  await expect(page.locator(`#${hiddenInputId}`)).toHaveValue(hhmm);
}
