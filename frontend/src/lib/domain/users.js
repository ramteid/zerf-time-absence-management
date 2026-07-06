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

export function userInitialsFromRows(userId, users) {
  return userInitials(findUserById(users, userId));
}

// CSS class per role for avatar background/text color (see .avatar-role-*
// in styles.css). Keeps avatar color consistent for a given user everywhere
// they appear, instead of depending on ad-hoc per-page inline styles.
const AVATAR_ROLE_CLASSES = {
  admin: "avatar-role-admin",
  team_lead: "avatar-role-team_lead",
  employee: "avatar-role-employee",
  assistant: "avatar-role-assistant",
};

export function userAvatarClass(user) {
  return AVATAR_ROLE_CLASSES[user?.role] || "";
}

export function userAvatarClassFromRows(userId, users) {
  return userAvatarClass(findUserById(users, userId));
}

// Display order for role-grouped user lists: team leads, then employees,
// then assistants, then admins.
const ROLE_SORT_ORDER = {
  team_lead: 0,
  employee: 1,
  assistant: 2,
  admin: 3,
};

// Sorts users into role groups (team lead, employee, assistant, admin),
// alphabetically by last/first name within each group. Use this wherever a
// roster of users is displayed, instead of a flat name-only sort.
export function sortUsersByRoleThenName(users) {
  return [...(users || [])].sort((a, b) => {
    const roleDiff =
      (ROLE_SORT_ORDER[a?.role] ?? 99) - (ROLE_SORT_ORDER[b?.role] ?? 99);
    if (roleDiff !== 0) return roleDiff;
    return (
      (a?.last_name || "").localeCompare(b?.last_name || "") ||
      (a?.first_name || "").localeCompare(b?.first_name || "")
    );
  });
}

export function userWorkdaysPerWeek(user, fallback = 5) {
  const value = Number(user?.workdays_per_week);
  return Number.isFinite(value) && value >= 1 && value <= 7 ? value : fallback;
}

export function userWorkdaysPerWeekById(users, userId, fallback = 5) {
  return userWorkdaysPerWeek(findUserById(users, userId), fallback);
}
