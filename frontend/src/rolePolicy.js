const ASSISTANT_ROLE = "assistant";

function roleOf(user) {
  return String(user?.role || "").toLowerCase();
}

export function isAssistantUser(user) {
  return roleOf(user) === ASSISTANT_ROLE;
}

export function hasFlextimeAccount(user) {
  return !!user && !isAssistantUser(user);
}
