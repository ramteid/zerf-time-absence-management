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

export function userWorkdaysPerWeek(user, fallback = 5) {
  const value = Number(user?.workdays_per_week);
  return Number.isFinite(value) && value >= 1 && value <= 7 ? value : fallback;
}

export function userWorkdaysPerWeekById(users, userId, fallback = 5) {
  return userWorkdaysPerWeek(findUserById(users, userId), fallback);
}
