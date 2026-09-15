import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";
import { api } from "./api.js";
import {
  markAllNotificationsRead,
  markNotificationRead,
  refreshNotifications,
  stopNotifications,
} from "./notificationService.js";
import { notifications, notificationsUnread } from "./stores.js";

vi.mock("./api.js", () => ({ api: vi.fn() }));

function deferred() {
  let resolve;
  const promise = new Promise((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

// One refresh issues two requests. Hand each its own deferred so a test can
// decide when — and in which order — overlapping refreshes come back.
function queueRefresh() {
  const list = deferred();
  const count = deferred();
  api
    .mockImplementationOnce(() => list.promise)
    .mockImplementationOnce(() => count.promise);
  return {
    resolveWith(items, unread) {
      list.resolve(items);
      count.resolve({ count: unread });
    },
  };
}

beforeEach(() => {
  api.mockReset();
  notifications.set([]);
  notificationsUnread.set(0);
});

describe("refreshNotifications", () => {
  it("ignores a slow response that a newer refresh has already overtaken", async () => {
    const slow = queueRefresh();
    const fast = queueRefresh();

    const slowRun = refreshNotifications();
    const fastRun = refreshNotifications();

    fast.resolveWith([{ id: 2 }], 2);
    await fastRun;
    slow.resolveWith([{ id: 1 }], 99);
    await slowRun;

    expect(get(notifications)).toEqual([{ id: 2 }]);
    expect(get(notificationsUnread)).toBe(2);
  });

  it("does not repopulate the stores after the session ended", async () => {
    const inFlight = queueRefresh();
    const run = refreshNotifications();

    stopNotifications();
    inFlight.resolveWith([{ id: 1 }], 5);
    await run;

    expect(get(notifications)).toEqual([]);
    expect(get(notificationsUnread)).toBe(0);
  });

  it("survives a response without a list", async () => {
    api.mockResolvedValueOnce(null).mockResolvedValueOnce({ count: 0 });

    await refreshNotifications();

    expect(get(notifications)).toEqual([]);
  });
});

describe("local mutations", () => {
  it("is not reverted by a refresh that was already in flight", async () => {
    notifications.set([{ id: 1, is_read: false }]);
    notificationsUnread.set(1);

    const inFlight = queueRefresh();
    api.mockResolvedValueOnce({ ok: true });

    const run = refreshNotifications();
    await markAllNotificationsRead();

    inFlight.resolveWith([{ id: 1, is_read: false }], 1);
    await run;

    expect(get(notificationsUnread)).toBe(0);
    expect(get(notifications)[0].is_read).toBe(true);
  });

  it("marks one notification read without mutating the caller's object", async () => {
    const item = { id: 7, is_read: false };
    notifications.set([item]);
    notificationsUnread.set(1);
    api.mockResolvedValueOnce({ ok: true });

    await markNotificationRead(item);

    expect(item.is_read).toBe(false);
    expect(get(notifications)[0].is_read).toBe(true);
    expect(get(notificationsUnread)).toBe(0);
  });
});
