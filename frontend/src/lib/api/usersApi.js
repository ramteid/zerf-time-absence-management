import { api } from "../../api.js";

export function getUsers() {
  return api("/users");
}

export function getUser(userId) {
  return api(`/users/${userId}`);
}
