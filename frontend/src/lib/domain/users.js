import {
  hasFlextimeAccount,
  isAssistantUser,
  isPureAdminUser,
  tracksOwnTime,
} from "../../rolePolicy.js";

export { hasFlextimeAccount, isAssistantUser, isPureAdminUser, tracksOwnTime };

// Filters out users who don't track their own time (pure admins). Use this for
// any employee-selection dropdown that drives a report about a single user's
// own time or absences.
export function timeTrackingUsers(users) {
  return (users || []).filter((u) => {
    // tracksOwnTime treats tracks_time===undefined as tracking; we want to exclude undefined for dropdowns.
    if (u?.tracks_time === undefined) return false;
    return tracksOwnTime(u);
  });
}

export function findUserById(users, userId, fallbackUser = null) {
  const id = Number(userId);
  if (!Number.isFinite(id)) return fallbackUser || null;
  const found = (users || []).find((user) => Number(user?.id) === id);
  if (found) return found;
  const fallbackId = Number(fallbackUser?.id);
  if (Number.isFinite(fallbackId) && fallbackId === id) return fallbackUser;
  return null;
}

export function hasUserId(users, userId) {
  const id = Number(userId);
  return (
    Number.isFinite(id) && (users || []).some((user) => Number(user?.id) === id)
  );
}

export function userFullName(user, fallback = "") {
  if (!user) return fallback;
  const name = [user.first_name, user.last_name].filter(Boolean).join(" ");
  return name || fallback;
}

export function userNameFromRows(userId, users, fallback = `#${userId}`) {
  return userFullName(findUserById(users, userId), fallback);
}

export function userInitials(user) {
  return (
    (user?.first_name?.[0] || "") + (user?.last_name?.[0] || "")
  ).toUpperCase();
}

// Canonical role display order for user rosters: team leads, then employees,
// then assistants, then admins. This is the single source for both the
// role-grouped sort and the per-role avatar colour class.
//
// IMPORTANT: keep this in sync with the backend's `roles::role_sort_rank`
// (backend/src/roles.rs), which orders the combined timesheet PDF sections the
// same way. The order is pinned by tests on both sides (users.test.js here,
// role_sort_rank_orders_* in roles.rs).
const ROLE_ORDER = ["team_lead", "employee", "assistant", "admin"];

// Sort rank for a role. Unknown or absent roles sort last — e.g. the
// /team-users endpoint intentionally omits `role` for non-manageable
// colleagues, so those rows carry no role to group by.
function roleRank(role) {
  const index = ROLE_ORDER.indexOf(role);
  return index === -1 ? ROLE_ORDER.length : index;
}

// CSS class for a user's avatar background/text colour (see .avatar-role-*
// in styles.css). Keeps a user's avatar colour consistent everywhere they
// appear, instead of depending on ad-hoc per-page inline styles. Unknown or
// absent roles get no class and fall back to the neutral base .avatar style.
export function userAvatarClass(user) {
  return ROLE_ORDER.includes(user?.role) ? `avatar-role-${user.role}` : "";
}

// Null-safe last-name-then-first-name comparator — the name half of every
// user ordering in the app. Used directly where roles aren't available to
// group by (the Team Users list, where the server redacts colleagues' roles)
// and as the within-role tiebreak below.
export function compareUsersByName(a, b) {
  return (
    (a?.last_name || "").localeCompare(b?.last_name || "") ||
    (a?.first_name || "").localeCompare(b?.first_name || "")
  );
}

// Comparator that groups users by role (team lead, employee, assistant,
// admin), alphabetically by name within each group. Exposed so callers that
// sort something other than a plain user array (e.g. absence rows keyed by
// user_id) can reuse the exact same ordering.
export function compareUsersByRoleThenName(a, b) {
  const roleDiff = roleRank(a?.role) - roleRank(b?.role);
  if (roleDiff !== 0) return roleDiff;
  return compareUsersByName(a, b);
}

// Sorts a roster into role groups, alphabetical within each group. Use this
// wherever a list of users is displayed, instead of a flat name-only sort.
export function sortUsersByRoleThenName(users) {
  return [...(users || [])].sort(compareUsersByRoleThenName);
}

// Comparator for the /team-users roster: the endpoint redacts `role` for
// colleagues the requesting lead can't manage, so `can_manage` (assistant vs.
// not) is the only grouping signal available. Non-manageable colleagues sort
// first, manageable assistants after — matching where "assistant" falls in
// ROLE_ORDER above — alphabetical by name within each group.
export function compareTeamUserRows(a, b) {
  const manageDiff = (a?.can_manage ? 1 : 0) - (b?.can_manage ? 1 : 0);
  return manageDiff || compareUsersByName(a, b);
}

export function userWorkdaysPerWeek(user, fallback = 5) {
  const value = Number(user?.workdays_per_week);
  return Number.isFinite(value) && value >= 1 && value <= 7 ? value : fallback;
}
