import { get } from "svelte/store";
import { api } from "./api.js";
import { t } from "./i18n.js";
import {
  categories,
  absenceCategories,
  earliestStartDate,
  toast,
} from "./stores.js";

/**
 * Load the per-user data that only becomes reachable once a user is past the
 * `must_change_password` gate: the earliest start date (report date-picker
 * bounds), time categories, and absence categories. The auth middleware 403s
 * these endpoints while a temporary password is in force, so boot deliberately
 * waits until the gate clears before calling them.
 *
 * This is the single source of truth for both entry points that need it:
 *   - boot, via App.svelte `loadMe`, once `must_change_password` is false; and
 *   - the first-login password change, via Account.svelte `changePassword`,
 *     which lifts that gate mid-session.
 *
 * Keeping it in one place prevents the two paths from drifting. Previously they
 * were duplicated and did drift: boot was extended to load absence categories
 * but the password-change path was not, leaving freshly created users with an
 * empty absence-category dropdown for the rest of their session.
 *
 * Each store is fetched only when still unset, so a redundant call (e.g. a
 * normal user changing their password, whose stores boot already populated)
 * issues no requests and never clobbers data a later navigation loaded.
 * Failures are non-fatal: the affected feature degrades until the next load
 * rather than blocking the caller.
 */
export async function loadPostAuthData() {
  if (!get(earliestStartDate)) {
    try {
      const { earliest_start_date } = await api("/users/earliest-start-date");
      earliestStartDate.set(earliest_start_date ?? null);
    } catch {
      // Non-fatal: report date pickers fall back to no lower bound.
    }
  }
  if (!get(categories).length) {
    try {
      categories.set(await api("/categories"));
    } catch {
      toast(
        get(t)("Failed to load categories. Some features may be unavailable."),
        "error",
      );
    }
  }
  if (!get(absenceCategories).length) {
    try {
      absenceCategories.set(await api("/absence-categories"));
    } catch {
      // Non-fatal: the absence-request dropdown stays empty until reload.
    }
  }
}
