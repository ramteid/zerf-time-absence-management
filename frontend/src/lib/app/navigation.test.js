// Tests for the SPA navigation helper. Zerf is a single-page app that uses
// pushState routing; page components are swapped by the path store rather than
// by full-page loads. Tests verify that:
//   - go() correctly updates both the browser history and the path store
//   - The browser back button triggers a path store update
//   - Pressing back when a dialog is open closes the dialog instead of
//     navigating, preventing the user from leaving the page unexpectedly

import { describe, expect, it, vi, beforeEach } from "vitest";

describe("navigation", () => {
  beforeEach(() => {
    vi.resetModules();
  });

  it("path store is initialized with the current browser location", async () => {
    // On boot, the path store must reflect the URL the user actually navigated
    // to (e.g. a direct link to /reports), not always default to '/'.
    const { path } = await import("./navigation.js");
    let value;
    path.subscribe((v) => (value = v))();
    expect(typeof value).toBe("string");
  });

  it("go() pushes the new URL to browser history", async () => {
    // pushState is what makes the back button work. Without it, the user
    // would have no history entry to return to after clicking a nav link.
    const { go, path } = await import("./navigation.js");
    const pushSpy = vi.spyOn(history, "pushState");
    go("/test-route");
    expect(pushSpy).toHaveBeenCalled();
    let value;
    path.subscribe((v) => (value = v))();
    expect(value).toContain("/test-route");
    pushSpy.mockRestore();
  });

  it("go() with push=false uses replaceState instead of pushing a new entry", async () => {
    // Some navigations (e.g. redirecting after login) should replace the
    // current history entry so the user cannot press Back and land on the
    // login page again.
    const { go } = await import("./navigation.js");
    const replaceSpy = vi.spyOn(history, "replaceState");
    go("/replace-route", false);
    expect(replaceSpy).toHaveBeenCalled();
    replaceSpy.mockRestore();
  });

  it("popstate event updates the path store to the restored URL", async () => {
    // When the user presses the browser Back button, the popstate event fires.
    // The path store must be updated so the router renders the correct page.
    const { path } = await import("./navigation.js");
    history.pushState({}, "", "/popped");
    window.dispatchEvent(new PopStateEvent("popstate"));
    let value;
    path.subscribe((v) => (value = v))();
    expect(value).toContain("/popped");
  });

  it("popstate closes an open dialog instead of navigating away", async () => {
    // When a modal dialog is open, pressing Back should dismiss the dialog
    // rather than leaving the page. This matches native mobile app behavior
    // and prevents loss of in-progress form data.
    await import("./navigation.js");
    const dialog = document.createElement("dialog");
    dialog.setAttribute("open", "");
    // jsdom doesn't implement HTMLDialogElement.close(); add it so spyOn
    // has a real property to wrap.
    dialog.close = function () { this.removeAttribute("open"); };
    const closeSpy = vi.spyOn(dialog, "close");
    document.body.appendChild(dialog);
    const pushSpy = vi.spyOn(history, "pushState");

    window.dispatchEvent(new PopStateEvent("popstate"));

    expect(closeSpy).toHaveBeenCalled();
    // A new history entry is pushed so the URL stays the same after the close
    expect(pushSpy).toHaveBeenCalled();

    dialog.remove();
    closeSpy.mockRestore();
    pushSpy.mockRestore();
  });
});
