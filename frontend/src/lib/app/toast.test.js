// Tests for the in-app toast notification system. Toasts provide brief
// confirmation or error feedback after user actions (e.g. "Settings saved.",
// "User deactivated."). Tests verify that:
//   - Messages appear in the store with the correct type
//   - Each toast has a unique ID so the UI can render multiple simultaneously
//   - Toasts auto-dismiss after the timeout so they don't clutter the screen

import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { toast, toasts } from "./toast.js";

describe("toast", () => {
  let values = [];

  beforeEach(() => {
    values = [];
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("adds a toast with the default 'info' type when no type is given", async () => {
    // Most informational messages (e.g. data loaded) use the info style.
    // The default prevents callers from having to pass a type every time.
    const unsub = toasts.subscribe((v) => (values = v));
    toast("Hello");
    expect(values.some((t) => t.message === "Hello" && t.type === "info")).toBe(true);
    unsub();
  });

  it("stores the caller-supplied type (e.g. 'error', 'ok')", async () => {
    // Error toasts use a different visual style (red) so users can tell at
    // a glance whether an action succeeded or failed. The type must be
    // preserved exactly as passed.
    const unsub = toasts.subscribe((v) => (values = v));
    toast("Error occurred", "error");
    expect(values.some((t) => t.message === "Error occurred" && t.type === "error")).toBe(true);
    unsub();
  });

  it("removes the toast automatically after 3500 ms", async () => {
    // Toasts are ephemeral feedback — they should not pile up permanently.
    // 3.5 seconds gives the user enough time to read a short message.
    const unsub = toasts.subscribe((v) => (values = v));
    toast("Temporary");
    const countBefore = values.length;
    vi.advanceTimersByTime(3500);
    expect(values.length).toBe(countBefore - 1);
    unsub();
  });

  it("assigns unique IDs so multiple concurrent toasts can be tracked individually", async () => {
    // If two toasts had the same ID, the auto-dismiss timeout for one would
    // also remove the other, leaving stale messages stuck on screen.
    const unsub = toasts.subscribe((v) => (values = v));
    toast("First");
    toast("Second");
    const ids = values.slice(-2).map((t) => t.id);
    expect(ids[0]).not.toBe(ids[1]);
    unsub();
  });
});
