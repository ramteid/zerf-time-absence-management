const ASSISTANT_ROLE = "assistant";
const ADMIN_ROLE = "admin";

function roleOf(user) {
  return String(user?.role || "").trim().toLowerCase();
}

export function isAssistantUser(user) {
  return roleOf(user) === ASSISTANT_ROLE;
}

export function hasFlextimeAccount(user) {
  return !!user && !isAssistantUser(user);
}

// Pure-admin users have role=admin and tracks_time=false. They have no time or
// absence data themselves but can still approve and view team-wide reports.
export function isPureAdminUser(user) {
  return !!user && roleOf(user) === ADMIN_ROLE && user.tracks_time === false;
}

// True for any user that records their own time and absences (so they belong
// in employee dropdowns / their own time-tracking views). False for pure-admin
// users, where time tracking has been disabled in the user settings.
export function tracksOwnTime(user) {
  return !!user && user.tracks_time !== false;
}
