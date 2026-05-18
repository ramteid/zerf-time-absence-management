import { api } from "../../api.js";

export function getPublicSettings() {
  return api("/settings/public");
}

export function getSettings() {
  return api("/settings");
}
