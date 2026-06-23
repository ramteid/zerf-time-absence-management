import { api } from "../../api.js";

export function getUsers() {
  return api("/users");
}

export function getUser(userId) {
  return api(`/users/${userId}`);
}

export function getArchivedUsers() {
  return api("/users/archived");
}

/**
 * Archive a user. When the user currently approves active team members,
 * `approverReplacements` must map each affected user_id (string) to the
 * replacement approver's id (number): { "42": 7, "99": 7 }.
 */
export function archiveUser(userId, approverReplacements = {}) {
  return api(`/users/${userId}/archive`, {
    method: "POST",
    body: { approver_replacements: approverReplacements },
  });
}

/**
 * Restore an archived user.
 * @param {number}      userId
 * @param {string|null} startDate   ISO date string or null to keep original
 * @param {number[]}    approverIds Required for non-admin roles
 */
export function restoreUser(userId, startDate, approverIds) {
  return api(`/users/${userId}/restore`, {
    method: "POST",
    body: {
      start_date: startDate || null,
      approver_ids: approverIds || [],
    },
  });
}
