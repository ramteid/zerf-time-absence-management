// Tests for the session-broadcast helper that keeps all open tabs in sync.
// When a user logs out in one tab, the other tabs must detect the logout
// via the BroadcastChannel API and redirect. These tests verify that:
//   1. Messages are forwarded when BroadcastChannel is available.
//   2. The module degrades gracefully on browsers/environments without BroadcastChannel
//      (e.g. older Firefox, some jsdom environments) rather than throwing.

import { describe, expect, it, vi } from "vitest";

describe("sessionBroadcast", () => {
  it("broadcasts a message and delivers it to registered listeners", async () => {
    // Simulates the happy path: Tab A calls broadcastSession("logout"),
    // Tab B (same test, different listener) receives { type: "logout" }.
    // The mock captures postMessage calls and replays them to listeners.
    const listeners = new Map();
    const mockChannel = {
      postMessage: vi.fn(),
      addEventListener: vi.fn((event, handler) => {
        if (!listeners.has(event)) listeners.set(event, []);
        listeners.get(event).push(handler);
      }),
      removeEventListener: vi.fn((event, handler) => {
        if (listeners.has(event)) {
          listeners.set(
            event,
            listeners.get(event).filter((h) => h !== handler),
          );
        }
      }),
    };

    const OriginalBC = globalThis.BroadcastChannel;
    // Arrow functions can't be used as constructors; use a regular function
    // so 'new BroadcastChannel(...)' returns the mockChannel object.
    globalThis.BroadcastChannel = function () { return mockChannel; };

    vi.resetModules();
    const { broadcastSession, onSessionBroadcast } = await import(
      "./sessionBroadcast.js"
    );

    const received = [];
    const off = onSessionBroadcast((data) => received.push(data));

    broadcastSession("logout");
    expect(mockChannel.postMessage).toHaveBeenCalledWith({ type: "logout" });

    // Replay the incoming event so we can verify the listener is called
    const handler = listeners.get("message")?.[0];
    if (handler) handler({ data: { type: "login" } });
    expect(received).toEqual([{ type: "login" }]);

    // Verify that unsubscribing removes the event listener to prevent leaks
    off();
    expect(mockChannel.removeEventListener).toHaveBeenCalled();

    globalThis.BroadcastChannel = OriginalBC;
    vi.resetModules();
  });

  it("returns a harmless no-op when BroadcastChannel is unavailable", async () => {
    // Some embedded webviews and older environments do not support
    // BroadcastChannel. The module must not crash the app; instead, cross-tab
    // sync is simply skipped. The returned teardown function must also be safe
    // to call so callers do not need to guard against undefined.
    const OriginalBC = globalThis.BroadcastChannel;
    delete globalThis.BroadcastChannel;

    vi.resetModules();
    const { onSessionBroadcast, broadcastSession } = await import(
      "./sessionBroadcast.js"
    );

    const off = onSessionBroadcast(() => {});
    expect(typeof off).toBe("function");
    expect(() => off()).not.toThrow();
    expect(() => broadcastSession("test")).not.toThrow();

    globalThis.BroadcastChannel = OriginalBC;
    vi.resetModules();
  });
});
