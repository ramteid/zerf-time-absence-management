import { api } from "../../api.js";

export function getPublicSettings() {
  return api("/settings/public");
}

export function getSettings() {
  return api("/settings");
}

export function updateUploadSettings(body) {
  return api("/settings/uploads", { method: "PUT", body });
}

export function runReportUploadNow() {
  return api("/settings/uploads/report/run-now", { method: "POST" });
}
