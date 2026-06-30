import { defineConfig, devices } from "@playwright/test";

// The bash orchestrator (run.sh) boots the Docker stack and exports BASE_URL.
// When a developer runs `npx playwright test` directly against an already-running
// stack, BASE_URL falls back to the local-mode default published by start_local.sh.
const baseURL = process.env.BASE_URL || "http://localhost:3333";

export default defineConfig({
  testDir: "./tests",
  // The full flow (admin setup → user creation → bookings → approvals) is one
  // long linear scenario, so the suite runs strictly serially in a single worker.
  fullyParallel: false,
  workers: 1,
  forbidOnly: !!process.env.CI,
  // One retry in CI smooths over the occasional first-boot timing hiccup; locally
  // a failure should surface immediately so it can be debugged.
  retries: process.env.CI ? 1 : 0,
  // The scenario walks through many screens; give each phase generous headroom.
  timeout: 90_000,
  expect: { timeout: 15_000 },
  reporter: [["list"], ["html", { open: "never" }]],
  use: {
    baseURL,
    headless: true,
    // Diagnostics are kept only when something goes wrong, so green runs stay light.
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    video: "retain-on-failure",
    // Local mode serves plain HTTP with non-secure cookies; nothing TLS-specific.
    ignoreHTTPSErrors: true,
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
});
