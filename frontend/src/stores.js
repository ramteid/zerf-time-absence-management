import { writable } from "svelte/store";
export { theme } from "./lib/app/theme.js";
export { path, go } from "./lib/app/navigation.js";
export { toast, toasts } from "./lib/app/toast.js";
export {
  broadcastSession,
  onSessionBroadcast,
} from "./lib/app/sessionBroadcast.js";

export const currentUser = writable(null);
export const categories = writable([]);
export const settings = writable({ ui_language: "en", time_format: "24h", timezone: "Europe/Berlin" });

// In-app notification center.
export const notifications = writable([]);
export const notificationsUnread = writable(0);
