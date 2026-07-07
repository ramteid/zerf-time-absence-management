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
  return (users || []).filter(tracksOwnTime);
}

export function findUserById(users, userId, fallbackUser = null) {
  const id = Number(userId);
  return (
    (users || []).find((user) => Number(user?.id) === id) ||
    (Number(fallbackUser?.id) === id ? fallbackUser : null)
  );
}

export function hasUserId(users, userId) {
  const id = Number(userId);
  return Number.isFinite(id) && (users || []).some((user) => Number(user?.id) === id);
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

// Comparator that groups users by role (team lead, employee, assistant,
// admin), alphabetically by last/first name within each group. Exposed so
// callers that sort something other than a plain user array (e.g. absence
// rows keyed by user_id) can reuse the exact same ordering.
export function compareUsersByRoleThenName(a, b) {
  const roleDiff = roleRank(a?.role) - roleRank(b?.role);
  if (roleDiff !== 0) return roleDiff;
  return (
    (a?.last_name || "").localeCompare(b?.last_name || "") ||
    (a?.first_name || "").localeCompare(b?.first_name || "")
  );
}

// Sorts a roster into role groups, alphabetical within each group. Use this
// wherever a list of users is displayed, instead of a flat name-only sort.
export function sortUsersByRoleThenName(users) {
  return [...(users || [])].sort(compareUsersByRoleThenName);
}

export function userWorkdaysPerWeek(user, fallback = 5) {
  const value = Number(user?.workdays_per_week);
  return Number.isFinite(value) && value >= 1 && value <= 7 ? value : fallback;
}

export function userWorkdaysPerWeekById(users, userId, fallback = 5) {
  return userWorkdaysPerWeek(findUserById(users, userId), fallback);
}
