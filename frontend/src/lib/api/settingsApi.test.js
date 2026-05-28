// Tests for the thin settingsApi wrappers that bridge the frontend to the
// backend /settings endpoints. The wrappers exist so route components import a
// named function instead of a raw URL string — if the URL ever changes, the
// fix stays in one place. Tests verify the exact URLs are forwarded correctly.

import { describe, expect, it, vi } from "vitest";

vi.mock("../../api.js", () => ({
  api: vi.fn(),
}));

import { api } from "../../api.js";
import { getPublicSettings, getSettings } from "./settingsApi.js";

describe("settingsApi", () => {
  it("getPublicSettings forwards to /settings/public", async () => {
    // Public settings (SMTP status, org name) are read-only and accessible
    // without full admin credentials. The wrapper must not hit the protected
    // /settings endpoint which requires admin auth.
    api.mockResolvedValue({ ui_language: "en" });
    const result = await getPublicSettings();
    expect(api).toHaveBeenCalledWith("/settings/public");
    expect(result).toEqual({ ui_language: "en" });
  });

  it("getSettings forwards to /settings (admin endpoint)", async () => {
    // Full admin settings include SMTP credentials and are guarded by the
    // admin role check on the backend. The wrapper must use the correct URL
    // so the auth middleware can enforce the role requirement.
    api.mockResolvedValue({ smtp_enabled: true });
    const result = await getSettings();
    expect(api).toHaveBeenCalledWith("/settings");
    expect(result).toEqual({ smtp_enabled: true });
  });
});
