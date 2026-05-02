import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { api, csrfToken } from "./api.js";

describe("api", () => {
  let fetchSpy;

  beforeEach(() => {
    csrfToken.set(null);
    fetchSpy = vi.spyOn(globalThis, "fetch");
  });

  afterEach(() => {
    fetchSpy.mockRestore();
  });

  it("makes GET request to /api/v1 prefix", async () => {
    fetchSpy.mockResolvedValue({
      ok: true,
      status: 200,
      headers: new Headers({ "content-type": "application/json" }),
      json: () => Promise.resolve({ data: 1 }),
    });

    const result = await api("/users");
    expect(result).toEqual({ data: 1 });
    expect(fetchSpy).toHaveBeenCalledWith(
      "/api/v1/users",
      expect.objectContaining({ credentials: "same-origin" }),
    );
  });

  it("returns null for 204 responses", async () => {
    fetchSpy.mockResolvedValue({
      ok: true,
      status: 204,
      headers: new Headers(),
    });

    const result = await api("/resource", { method: "DELETE" });
    expect(result).toBe(null);
  });

  it("throws on non-ok JSON responses", async () => {
    fetchSpy.mockResolvedValue({
      ok: false,
      status: 400,
      headers: new Headers({ "content-type": "application/json" }),
      json: () => Promise.resolve({ error: "Bad request" }),
    });

    await expect(api("/bad")).rejects.toThrow("Bad request");
  });

  it("throws on non-ok text responses", async () => {
    fetchSpy.mockResolvedValue({
      ok: false,
      status: 500,
      headers: new Headers({ "content-type": "text/plain" }),
      text: () => Promise.resolve("Internal Server Error"),
    });

    await expect(api("/fail")).rejects.toThrow("Internal Server Error");
  });

  it("sends JSON body for POST", async () => {
    fetchSpy.mockResolvedValue({
      ok: true,
      status: 200,
      headers: new Headers({ "content-type": "application/json" }),
      json: () => Promise.resolve({ id: 1 }),
    });

    await api("/create", { method: "POST", body: { name: "test" } });
    const call = fetchSpy.mock.calls[0];
    expect(call[1].body).toBe('{"name":"test"}');
    expect(call[1].headers["Content-Type"]).toBe("application/json");
  });

  it("includes CSRF token on mutating requests", async () => {
    csrfToken.set("tok123");
    fetchSpy.mockResolvedValue({
      ok: true,
      status: 200,
      headers: new Headers({ "content-type": "application/json" }),
      json: () => Promise.resolve({}),
    });

    await api("/update", { method: "POST", body: { x: 1 } });
    const headers = fetchSpy.mock.calls[0][1].headers;
    expect(headers["X-CSRF-Token"]).toBe("tok123");
  });

  it("does not include CSRF token on GET requests", async () => {
    csrfToken.set("tok123");
    fetchSpy.mockResolvedValue({
      ok: true,
      status: 200,
      headers: new Headers({ "content-type": "application/json" }),
      json: () => Promise.resolve({}),
    });

    await api("/read");
    const headers = fetchSpy.mock.calls[0][1].headers;
    expect(headers["X-CSRF-Token"]).toBeUndefined();
  });

  it("returns raw response for non-JSON ok responses", async () => {
    const mockResponse = {
      ok: true,
      status: 200,
      headers: new Headers({ "content-type": "text/csv" }),
    };
    fetchSpy.mockResolvedValue(mockResponse);

    const result = await api("/export");
    expect(result).toBe(mockResponse);
  });
});
