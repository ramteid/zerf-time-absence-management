import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import AdminPayrollReport from "./AdminPayrollReport.svelte";
import { setLanguage } from "../i18n.js";

const mockState = vi.hoisted(() => ({
  settings: {
    payroll_report_enabled: true,
    payroll_report_recipients: ["payroll@example.com"],
    payroll_report_day_of_month: 5,
    // Read-only: which categories the backend currently includes
    // automatically (sick-like, or costing neither vacation nor flextime).
    payroll_report_absence_categories: ["sick"],
    payroll_report_include_assistant_hours: true,
    payroll_report_include_employee_hours: false,
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
}));

const toastMock = vi.hoisted(() => vi.fn());

// What POST /settings/payroll-report/send-now reports back for the next call.
const sendNowResult = vi.hoisted(() => ({ value: { sent: 0, pending: 1 } }));

const apiMock = vi.hoisted(() =>
  vi.fn(async (path, opts = {}) => {
    if (path === "/settings" && (!opts.method || opts.method === "GET")) {
      return mockState.settings;
    }
    if (path === "/absence-categories/all") {
      return mockState.categories;
    }
    if (path === "/settings/payroll-report" && opts.method === "PUT") {
      mockState.settings = { ...mockState.settings, ...opts.body };
      return mockState.settings;
    }
    if (
      path === "/settings/payroll-report/send-now" &&
      opts.method === "POST"
    ) {
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

    // No manual picker — nothing to check or toggle here.
    expect(target.querySelectorAll('input[type="checkbox"]').length).toBe(3);
    expect(target.textContent).toContain("Sick");
    expect(target.textContent).not.toContain("Unpaid");
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
      "A recipient address is required to enable the payroll report.",
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
      "Select at least one section for the payroll report.",
      "error",
    );
  });

  it("confirms a send only when a report actually went out", async () => {
    sendNowResult.value = { sent: 1, pending: 0 };
    component = mount(AdminPayrollReport, { target });
    await settle();

    clickButton(target, "Send now");
    await settle();

    expect(
      apiMock.mock.calls.some(
        ([path, opts]) =>
          path === "/settings/payroll-report/send-now" &&
          opts?.method === "POST",
      ),
    ).toBe(true);
    expect(toastMock).toHaveBeenCalledWith("Payroll report sent.", "ok");
  });

  it("explains when nothing was sent because a month is not final", async () => {
    sendNowResult.value = { sent: 0, pending: 1 };
    component = mount(AdminPayrollReport, { target });
    await settle();

    clickButton(target, "Send now");
    await settle();

    expect(toastMock).toHaveBeenCalledWith(
      "Nothing was sent: every month was already sent or is not final yet.",
      "info",
    );
  });
});
