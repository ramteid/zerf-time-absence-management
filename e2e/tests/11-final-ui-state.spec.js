// File 11: verifies the cumulative outcome of every approve / reject /
// cancel / reopen-reject decision *through the UI of the affected user* — the
// persistent status the employee and assistant actually see, not the
// transient toast the reviewer saw at the moment of acting, and explicitly
// not the audit log. This is the "additionally via the UI" confirmation that
// the whole approval chain (specs 06, 08, 10) genuinely changed state.
//
// It must run after 10 (the last review) and before 12 (which archives the
// employee, invalidating her session), hence the file number.

import { test, expect } from "@playwright/test";
import { storageStatePath } from "./helpers.js";
import { ADMIN, ASSISTANT, EMPLOYEE, TEAM_LEAD } from "./users.js";

test.describe("employee sees the final state of their items", () => {
  test.use({ storageState: storageStatePath("employee") });

  test("employee: approved week stayed approved after the reopen was rejected", async ({
    page,
  }) => {
    await page.goto("/time");
    await page.locator(".time-week-picker button").first().click();
    await expect(page.locator(".week-grid")).toBeVisible();
    // The reopen request was *rejected* in 10, so per the user guide the
    // week is left untouched: its entries are still "Approved", never reset
    // to draft. Seeing the Approved chips here (rather than editable draft
    // blocks) is the employee-side UI proof of that rejection's effect.
    await expect(
      page.locator(".week-grid").getByText("Approved").first(),
    ).toBeVisible();
  });

  test("employee: the approved vacation now shows as cancelled", async ({
    page,
  }) => {
    await page.goto("/absences");
    // Full lifecycle visible in one row's status chip: requested (05) →
    // approved (06) → cancellation requested (09) → cancellation approved
    // (10) → "Cancelled". The day-off request stayed "Rejected" (06).
    await expect(
      page.locator(".absence-entry", { hasText: "E2E vacation" }),
    ).toContainText("Cancelled");
    await expect(
      page.locator(".absence-entry", { hasText: "E2E day off" }),
    ).toContainText("Rejected");
  });
});

test.describe("assistant sees the final state of their items", () => {
  test.use({ storageState: storageStatePath("assistant") });

  test("assistant: week and absence both show as approved", async ({ page }) => {
    await page.goto("/time");
    await page.locator(".time-week-picker button").first().click();
    await expect(page.locator(".week-grid")).toBeVisible();
    await expect(
      page.locator(".week-grid").getByText("Approved").first(),
    ).toBeVisible();

    await page.goto("/absences");
    await expect(
      page.locator(".absence-entry", { hasText: "E2E assistant absence" }),
    ).toContainText("Approved");
  });
});

// By this point in the suite all four roles exist and none are archived yet
// (spec 12 archives the employee), so the admin's Users roster is the ideal
// place to confirm the two role-presentation behaviors end to end: every user
// list is grouped by role, and each avatar is colored by role.
test.describe("admin sees a role-grouped, role-colored user list", () => {
  test.use({ storageState: storageStatePath("admin") });

  test("admin: Users list is grouped by role, each avatar colored by role", async ({
    page,
  }) => {
    await page.goto("/settings/users");

    // The first card in the content area is the active-users roster (a second
    // card only appears once someone is archived, which hasn't happened yet).
    // Scope everything to this card: the same names/role classes also appear in
    // the sidebar account footer (e.g. the signed-in admin's own avatar), so an
    // unscoped page-wide lookup would be ambiguous.
    const roster = page.locator(".content-area .zf-card").first();

    // Wait until the roster has rendered a row for every role. Each assertion
    // also confirms that role's color class is present exactly where expected.
    await expect(roster.locator(".avatar-role-team_lead")).toBeVisible();
    await expect(roster.locator(".avatar-role-employee")).toBeVisible();
    await expect(roster.locator(".avatar-role-assistant")).toBeVisible();
    await expect(roster.locator(".avatar-role-admin")).toBeVisible();

    // Extract, in DOM (== visual) order, each row's name and the CSS classes on
    // its avatar chip. Row layout is [avatar][name+role block][action buttons],
    // so the name is the first child of the avatar's next sibling.
    const rows = await roster.evaluate((card) =>
      [...card.children].map((row) => {
        const avatar = row.querySelector(".avatar");
        const nameLine = avatar?.nextElementSibling?.firstElementChild;
        return {
          name: (nameLine?.textContent || "").replace(/\s+/g, " ").trim(),
          avatarClass: avatar?.className || "",
        };
      }),
    );

    const indexOf = (u) =>
      rows.findIndex((r) => r.name === `${u.firstName} ${u.lastName}`);
    const lead = indexOf(TEAM_LEAD);
    const employee = indexOf(EMPLOYEE);
    const assistant = indexOf(ASSISTANT);
    const admin = indexOf(ADMIN);

    // All four are present (findIndex would return -1 otherwise)...
    expect(Math.min(lead, employee, assistant, admin)).toBeGreaterThanOrEqual(0);
    // ...and grouped in role order: team lead, employee, assistant, admin.
    expect(lead).toBeLessThan(employee);
    expect(employee).toBeLessThan(assistant);
    expect(assistant).toBeLessThan(admin);

    // Each user's avatar carries its own role color class.
    expect(rows[lead].avatarClass).toContain("avatar-role-team_lead");
    expect(rows[employee].avatarClass).toContain("avatar-role-employee");
    expect(rows[assistant].avatarClass).toContain("avatar-role-assistant");
    expect(rows[admin].avatarClass).toContain("avatar-role-admin");
  });
});
