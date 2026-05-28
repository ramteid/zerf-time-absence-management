// Tests for the theme store that persists the light/dark mode preference.
// The theme must survive page reloads (localStorage) and be applied immediately
// on boot so there is no flash of the wrong theme. Tests also verify graceful
// degradation when localStorage is blocked by browser privacy settings.

import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";

describe("theme store", () => {
  let originalGetItem;
  let originalSetItem;
  let store = {};

  beforeEach(() => {
    store = {};
    originalGetItem = Storage.prototype.getItem;
    originalSetItem = Storage.prototype.setItem;
    Storage.prototype.getItem = vi.fn((key) => store[key] ?? null);
    Storage.prototype.setItem = vi.fn((key, value) => {
      store[key] = value;
    });
    vi.resetModules();
  });

  afterEach(() => {
    Storage.prototype.getItem = originalGetItem;
    Storage.prototype.setItem = originalSetItem;
  });

  it("defaults to 'light' when no preference is stored", async () => {
    // A fresh install or a user who has never toggled dark mode should see
    // the light theme, not an arbitrary or browser-default theme.
    const { theme } = await import("./theme.js");
    let value;
    theme.subscribe((v) => (value = v))();
    expect(value).toBe("light");
  });

  it("reads the previously saved theme from localStorage on startup", async () => {
    // After the user chooses dark mode and refreshes, the stored preference
    // must be honoured immediately — before any reactive updates fire — to
    // prevent a flash of the light theme.
    store["zerf.theme"] = "dark";
    const { theme } = await import("./theme.js");
    let value;
    theme.subscribe((v) => (value = v))();
    expect(value).toBe("dark");
  });

  it("persists the new theme to localStorage when set() is called", async () => {
    // The user's preference must survive a page refresh. If localStorage is
    // not written, the next boot will always revert to the default 'light'.
    const { theme } = await import("./theme.js");
    theme.set("dark");
    expect(Storage.prototype.setItem).toHaveBeenCalledWith("zerf.theme", "dark");
    let value;
    theme.subscribe((v) => (value = v))();
    expect(value).toBe("dark");
  });

  it("applies the data-theme attribute to <html> so CSS variables take effect", async () => {
    // All color tokens (--bg-surface, --text-primary, etc.) switch via the
    // data-theme attribute on <html>. If this attribute is not updated,
    // toggling dark mode in the account panel would have no visible effect.
    const { theme } = await import("./theme.js");
    theme.set("dark");
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
    theme.set("light");
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
  });

  it("falls back to 'light' when localStorage throws (e.g. privacy mode)", async () => {
    // Browsers in strict privacy mode or iframe sandboxes may throw a
    // SecurityError on localStorage access. The app must not crash and must
    // still render with the default light theme.
    Storage.prototype.getItem = vi.fn(() => {
      throw new Error("SecurityError");
    });
    const { theme } = await import("./theme.js");
    let value;
    theme.subscribe((v) => (value = v))();
    expect(value).toBe("light");
  });
});
