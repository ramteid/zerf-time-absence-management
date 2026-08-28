import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import AdminPayrollReport from "./AdminPayrollReport.svelte";
import { localizeErrorMessage, setLanguage } from "../i18n.js";

const mockState = vi.hoisted(() => ({
  settings: {
    payroll_report_enabled: true,
    payroll_report_recipients: ["payroll@example.com"],
    payroll_report_day_of_month: 5,
    // Read-only: categories the backend includes automatically.
    payroll_report_absence_categories: ["sick"],
    payroll_report_include_assistant_hours: true,
    payroll_report_include_employee_hours: false,
    // The report requires email to be set up; on by default here so the
    // existing save tests aren't all about this one precondition.
    smtp_enabled: true,
    // The month "Send now" targets, as picked by the backend.
    payroll_report_send_now_period: "2026-08",
  },
  categories: [
    { id: 1, slug: "sick", name: "Sick", color: "#ef4444", active: true },
    { id: 2, slug: "unpaid", name: "Unpaid", color: "#64748b", active: true },
    {
      id: 3,
      slug: "old_leave",
      name: "Old leave",
      color: "#0ea5e9",
      active: false,
    },
  ],
  // Candidates for the exclusion list. The admin and the deactivated employee
  // must never be offered — neither can appear in the payroll report.
  users: [
    {
      id: 1,
      first_name: "Ada",
      last_name: "Admin",
      role: "admin",
      active: true,
    },
    {
      id: 2,
      first_name: "Erin",
      last_name: "Employee",
      role: "employee",
      active: true,
    },
    {
      id: 3,
      first_name: "Alex",
      last_name: "Assistant",
      role: "assistant",
      active: true,
    },
    {
      id: 4,
      first_name: "Dana",
      last_name: "Deactivated",
      role: "employee",
      active: false,
    },
  ],
}));

const toastMock = vi.hoisted(() => vi.fn());

// What POST /settings/payroll-report/send-now reports back for the next call.
const sendNowResult = vi.hoisted(() => ({
  value: { sent: 0, pending: 1, period: "2026-08" },
}));

// Makes the next send-now call reject the way the real API does on a failure.
const sendNowFails = vi.hoisted(() => ({ value: false }));

const apiMock = vi.hoisted(() =>
  vi.fn(async (path, opts = {}) => {
    if (path === "/settings" && (!opts.method || opts.method === "GET")) {
      return mockState.settings;
    }
    if (path === "/absence-categories/all") {
      return mockState.categories;
    }
    if (path === "/users") {
      return mockState.users;
    }
    if (path === "/settings/payroll-report" && opts.method === "PUT") {
      mockState.settings = { ...mockState.settings, ...opts.body };
      return mockState.settings;
    }
    if (
      path === "/settings/payroll-report/send-now" &&
      opts.method === "POST"
    ) {
      if (sendNowFails.value) {
        throw new Error(
          localizeErrorMessage("PAYROLL_SEND_FAILED:connection refused"),
        );
      }
      return sendNowResult.value;
    }
    throw new Error(`Unhandled API path: ${path}`);
  }),
);

vi.mock("svelte", async () => {
  return await import("../../node_modules/svelte/src/index-client.js");
});

vi.mock("../api.js", () => ({
  api: apiMock,
}));

vi.mock("../stores.js", async () => {
  const actual = await vi.importActual("../stores.js");
  return { ...actual, toast: toastMock };
});

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

function clickButton(target, label) {
  const button = [...target.querySelectorAll("button")].find(
    (candidate) => candidate.textContent.trim() === label,
  );
  expect(button, `button "${label}" not found`).toBeTruthy();
  button.click();
  return button;
}

describe("AdminPayrollReport", () => {
  let component;
  let target;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    setLanguage("en");
    apiMock.mockClear();
    toastMock.mockClear();
    mockState.settings = {
      payroll_report_enabled: true,
      payroll_report_recipients: ["payroll@example.com"],
      payroll_report_day_of_month: 5,
      payroll_report_absence_categories: ["sick"],
      payroll_report_include_assistant_hours: true,
      payroll_report_include_employee_hours: false,
      payroll_report_excluded_user_ids: [],
      smtp_enabled: true,
      payroll_report_send_now_period: "2026-08",
    };
  });

  afterEach(() => {
    if (component) {
      unmount(component);
      component = null;
    }
    target.remove();
  });

  it("shows the automatically included categories as a read-only list", async () => {
    component = mount(AdminPayrollReport, { target });
    await settle();

    // No manual category picker: the only checkboxes are "send automatically",
    // the two hours sections, and one per selectable person in the exclusion
    // list (admins and deactivated accounts are not offered).
    expect(target.querySelectorAll('input[type="checkbox"]').length).toBe(5);
    expect(target.textContent).toContain("Sick");
    expect(target.textContent).not.toContain("Unpaid");
  });

  it("offers only active non-admins in the exclusion list", async () => {
    component = mount(AdminPayrollReport, { target });
    await settle();

    // Collapse the template's indentation so names read as written.
    const list = target
      .querySelector(".check-list")
      .textContent.replace(/\s+/g, " ");
    expect(list).toContain("Erin Employee");
    expect(list).toContain("Alex Assistant");
    // Admins never appear in the report, so excluding them is meaningless.
    expect(list).not.toContain("Ada Admin");
    // Deactivated accounts must not be shown at all.
    expect(list).not.toContain("Dana Deactivated");
    expect(target.textContent).toContain("2 of 2 people included");
  });

  it("saves ticked people as the excluded list", async () => {
    mockState.settings.payroll_report_excluded_user_ids = [3];
    component = mount(AdminPayrollReport, { target });
    await settle();

    // The stored exclusion is reflected back into the list on load.
    const assistantBox = [
      ...target.querySelectorAll('.check-list input[type="checkbox"]'),
    ].find((box) => box.value === "3");
    expect(assistantBox.checked).toBe(true);
    expect(target.textContent).toContain("1 of 2 people included");

    clickButton(target, "Save");
    await settle();

    const saveCall = apiMock.mock.calls.find(
      ([path, opts]) =>
        path === "/settings/payroll-report" && opts?.method === "PUT",
    );
    expect(saveCall[1].body.payroll_report_excluded_user_ids).toEqual([3]);
  });

  it("shows concise German settings text", async () => {
    setLanguage("de");
    component = mount(AdminPayrollReport, { target });
    await settle();

    expect(target.textContent).toContain("Lohnmeldung automatisch senden");
    expect(target.textContent).toContain("Versandtag (1-28)");
    expect(target.textContent).toContain(
      "Sind noch Wochen, Abwesenheiten oder Arbeitszeiten offen",
    );
    expect(target.textContent).not.toContain("am konfigurierten Tag");
  });

  it("prefills the recipients field from the stored list", async () => {
    mockState.settings.payroll_report_recipients = [
      "payroll@example.com",
      "second@example.com",
    ];
    component = mount(AdminPayrollReport, { target });
    await settle();

    const input = target.querySelector("#payroll-recipients");
    expect(input.value).toBe("payroll@example.com\nsecond@example.com");
  });

  it("saves every configured field, parsing recipients from the input", async () => {
    component = mount(AdminPayrollReport, { target });
    await settle();

    const input = target.querySelector("#payroll-recipients");
    input.value =
      "payroll@example.com\n second@example.com \nsecond@example.com\n";
    input.dispatchEvent(new Event("input"));
    await settle();

    clickButton(target, "Save");
    await settle();

    const saveCall = apiMock.mock.calls.find(
      ([path, opts]) =>
        path === "/settings/payroll-report" && opts?.method === "PUT",
    );
    expect(saveCall).toBeTruthy();
    const body = saveCall[1].body;
    expect(body.payroll_report_enabled).toBe(true);
    expect(body.payroll_report_recipients).toEqual([
      "payroll@example.com",
      "second@example.com",
    ]);
    expect(body.payroll_report_day_of_month).toBe(5);
    expect(body.payroll_report_include_assistant_hours).toBe(true);
    expect(body.payroll_report_include_employee_hours).toBe(false);
  });

  it("refuses to enable the report without a recipient", async () => {
    mockState.settings = {
      ...mockState.settings,
      payroll_report_recipients: [],
    };
    component = mount(AdminPayrollReport, { target });
    await settle();

    clickButton(target, "Save");
    await settle();

    expect(
      apiMock.mock.calls.some(
        ([path, opts]) =>
          path === "/settings/payroll-report" && opts?.method === "PUT",
      ),
    ).toBe(false);
    expect(toastMock).toHaveBeenCalledWith(
      "Enter at least one recipient.",
      "error",
    );
  });

  it("refuses to enable the report when no section is selected", async () => {
    mockState.settings = {
      ...mockState.settings,
      payroll_report_absence_categories: [],
      payroll_report_include_assistant_hours: false,
      payroll_report_include_employee_hours: false,
    };
    component = mount(AdminPayrollReport, { target });
    await settle();

    clickButton(target, "Save");
    await settle();

    expect(
      apiMock.mock.calls.some(
        ([path, opts]) =>
          path === "/settings/payroll-report" && opts?.method === "PUT",
      ),
    ).toBe(false);
    expect(toastMock).toHaveBeenCalledWith(
      "Select at least one type of content.",
      "error",
    );
  });

  it("names the targeted month on the send button", async () => {
    component = mount(AdminPayrollReport, { target });
    await settle();

    expect(target.textContent).toContain("Send August 2026 now");
  });

  it("confirms a send, naming the month that went out", async () => {
    sendNowResult.value = { sent: 1, pending: 0, period: "2026-08" };
    component = mount(AdminPayrollReport, { target });
    await settle();

    clickButton(target, "Send August 2026 now");
    await settle();

    expect(
      apiMock.mock.calls.some(
        ([path, opts]) =>
          path === "/settings/payroll-report/send-now" &&
          opts?.method === "POST",
      ),
    ).toBe(true);
    expect(toastMock).toHaveBeenCalledWith("August 2026 sent.", "ok");
  });

  it("says so when the month had nothing to send", async () => {
    sendNowResult.value = {
      sent: 0,
      pending: 1,
      period: "2026-08",
      skipped: "nothing_approved",
    };
    component = mount(AdminPayrollReport, { target });
    await settle();

    clickButton(target, "Send August 2026 now");
    await settle();

    expect(toastMock).toHaveBeenCalledWith(
      "Nothing to send for August 2026 — no approved times yet.",
      "info",
    );
  });

  // "Nobody has finished the month" and "nothing is approved yet" are
  // different problems with different fixes, so the message must not collapse
  // them into one catch-all.
  it("names the actual reason nothing was sent", async () => {
    sendNowResult.value = {
      sent: 0,
      pending: 1,
      period: "2026-08",
      skipped: "nobody_final",
    };
    component = mount(AdminPayrollReport, { target });
    await settle();

    clickButton(target, "Send August 2026 now");
    await settle();

    expect(toastMock).toHaveBeenCalledWith(
      "Nothing sent for August 2026 — nobody has finished the month.",
      "info",
    );
  });

  it("reports a failed send as an error", async () => {
    component = mount(AdminPayrollReport, { target });
    await settle();

    sendNowFails.value = true;
    clickButton(target, "Send August 2026 now");
    await settle();

    expect(toastMock).toHaveBeenCalledWith(
      "The payroll report could not be sent: connection refused",
      "error",
    );
    sendNowFails.value = false;
  });

  it("refuses to enable the report before email is set up", async () => {
    mockState.settings = { ...mockState.settings, smtp_enabled: false };
    component = mount(AdminPayrollReport, { target });
    await settle();

    clickButton(target, "Save");
    await settle();

    expect(
      apiMock.mock.calls.some(
        ([path, opts]) =>
          path === "/settings/payroll-report" && opts?.method === "PUT",
      ),
    ).toBe(false);
    expect(toastMock).toHaveBeenCalledWith(
      "Set up email before turning this on.",
      "error",
    );
  });
});
