import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { get } from "svelte/store";
import { currentUser, categories, settings, path, go, toasts, toast } from "./stores.js";

describe("store defaults", () => {
  it("currentUser defaults to null", () => {
    expect(get(currentUser)).toBe(null);
  });

  it("categories defaults to empty array", () => {
    expect(get(categories)).toEqual([]);
  });

  it("settings defaults with ui_language", () => {
    expect(get(settings)).toEqual({ ui_language: "en" });
  });

  it("path defaults to current location", () => {
    expect(typeof get(path)).toBe("string");
  });
});

describe("go", () => {
  it("updates path store and pushes history state", () => {
    const spy = vi.spyOn(history, "pushState");
    go("/test-route");
    expect(get(path)).toBe("/test-route");
    expect(spy).toHaveBeenCalledWith({}, "", "/test-route");
    spy.mockRestore();
  });

  it("replaceState when push=false", () => {
    const spy = vi.spyOn(history, "replaceState");
    go("/replaced", false);
    expect(get(path)).toBe("/replaced");
    expect(spy).toHaveBeenCalledWith({}, "", "/replaced");
    spy.mockRestore();
  });
});

describe("toast", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("adds a toast to the store", () => {
    toast("Hello", "info");
    const items = get(toasts);
    expect(items).toHaveLength(1);
    expect(items[0]).toMatchObject({ message: "Hello", type: "info" });
  });

  it("auto-removes toast after timeout", () => {
    toast("Temporary");
    expect(get(toasts).length).toBeGreaterThanOrEqual(1);
    vi.advanceTimersByTime(4000);
    const remaining = get(toasts).filter((t) => t.message === "Temporary");
    expect(remaining).toHaveLength(0);
  });

  it("assigns unique ids", () => {
    toast("First");
    toast("Second");
    const items = get(toasts);
    const ids = items.map((t) => t.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("defaults to info type", () => {
    toast("Default type");
    const items = get(toasts);
    const last = items[items.length - 1];
    expect(last.type).toBe("info");
  });
});
