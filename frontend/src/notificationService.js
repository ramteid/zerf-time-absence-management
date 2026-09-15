import { api } from "./api.js";
import { notifications, notificationsUnread } from "./stores.js";

let pollTimer = null;
let visibilityHandler = null;
let eventSource = null;
let reconnectTimer = null;
let active = false;

// Several triggers can have a refresh in flight at the same time: the poll
// timer, an SSE event, the tab becoming visible again, opening the bell. Each
// refresh claims an id and may only write if its claim is still the newest, so
// a slow response cannot overwrite fresher state. Anything that makes an
// in-flight answer stale claims a fresh id too — a local mutation, or the
// session ending, which would otherwise repopulate the stores after logout.
let latestRefreshId = 0;

function supersedeInFlightRefreshes() {
  latestRefreshId += 1;
  return latestRefreshId;
}

export async function refreshNotifications() {
  if (typeof document !== "undefined" && document.hidden) return;
  const refreshId = supersedeInFlightRefreshes();
  const [list, count] = await Promise.all([
    api("/notifications"),
    api("/notifications/unread-count"),
  ]);
  if (refreshId !== latestRefreshId) return;
  notifications.set(list ?? []);
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
    if (eventSource) {
      eventSource.close();
      eventSource = null;
    }
    startPolling();
    // Attempt to re-establish the SSE stream after a back-off period so
    // real-time delivery resumes once the connection recovers. Drop any
    // pending attempt first, so repeated errors cannot stack up timers that
    // each try to reconnect.
    if (reconnectTimer) clearTimeout(reconnectTimer);
    reconnectTimer = setTimeout(() => {
      reconnectTimer = null;
      if (active) startStream();
    }, 30_000);
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
  supersedeInFlightRefreshes();
  if (pollTimer) {
    clearInterval(pollTimer);
    pollTimer = null;
  }
  if (visibilityHandler && typeof document !== "undefined") {
    document.removeEventListener("visibilitychange", visibilityHandler);
    visibilityHandler = null;
  }
  if (reconnectTimer) {
    clearTimeout(reconnectTimer);
    reconnectTimer = null;
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
  supersedeInFlightRefreshes();
  notifications.update((arr) =>
    arr.map((item) =>
      item.id === notification.id ? { ...item, is_read: true } : item,
    ),
  );
  notificationsUnread.update((count) => Math.max(0, count - 1));
}

export async function markAllNotificationsRead() {
  await api("/notifications/read-all", { method: "POST", body: {} });
  supersedeInFlightRefreshes();
  notifications.update((arr) =>
    arr.map((item) => ({ ...item, is_read: true })),
  );
  notificationsUnread.set(0);
}

export async function clearNotifications() {
  await api("/notifications", { method: "DELETE" });
  supersedeInFlightRefreshes();
  notifications.set([]);
  notificationsUnread.set(0);
}
