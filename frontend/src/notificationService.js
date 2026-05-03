import { api } from "./api.js";
import { notifications, notificationsUnread } from "./stores.js";

let pollTimer = null;
let visibilityHandler = null;
let eventSource = null;
let active = false;

export async function refreshNotifications() {
  if (typeof document !== "undefined" && document.hidden) return;
  const [list, count] = await Promise.all([
    api("/notifications"),
    api("/notifications/unread-count"),
  ]);
  notifications.set(list);
  notificationsUnread.set(count?.count ?? 0);
}

function startPolling() {
  if (pollTimer) return;
  pollTimer = setInterval(() => {
    refreshNotifications().catch(() => {});
  }, 60_000);
  if (typeof document !== "undefined" && !visibilityHandler) {
    visibilityHandler = () => {
      if (!document.hidden) refreshNotifications().catch(() => {});
    };
    document.addEventListener("visibilitychange", visibilityHandler);
  }
}

function startStream() {
  if (eventSource || typeof EventSource === "undefined") return;
  eventSource = new EventSource("/api/v1/notifications/stream");
  eventSource.addEventListener("notification", () => {
    refreshNotifications().catch(() => {});
  });
  eventSource.onerror = () => {
    startPolling();
  };
}

export function startNotifications() {
  if (active) return;
  active = true;
  refreshNotifications().catch(() => {});
  startPolling();
  startStream();
}

export function stopNotifications() {
  active = false;
  if (pollTimer) {
    clearInterval(pollTimer);
    pollTimer = null;
  }
  if (visibilityHandler && typeof document !== "undefined") {
    document.removeEventListener("visibilitychange", visibilityHandler);
    visibilityHandler = null;
  }
  if (eventSource) {
    eventSource.close();
    eventSource = null;
  }
  notifications.set([]);
  notificationsUnread.set(0);
}

export async function markNotificationRead(notification) {
  if (notification.is_read) return;
  await api(`/notifications/${notification.id}/read`, {
    method: "POST",
    body: {},
  });
  notification.is_read = true;
  notifications.update((arr) => arr.slice());
  notificationsUnread.update((count) => Math.max(0, count - 1));
}

export async function markAllNotificationsRead() {
  await api("/notifications/read-all", { method: "POST", body: {} });
  notifications.update((arr) =>
    arr.map((item) => ({ ...item, is_read: true })),
  );
  notificationsUnread.set(0);
}

export async function clearNotifications() {
  await api("/notifications", { method: "DELETE" });
  notifications.set([]);
  notificationsUnread.set(0);
}
