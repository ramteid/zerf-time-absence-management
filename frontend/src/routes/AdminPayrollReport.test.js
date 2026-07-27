import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import AdminPayrollReport from "./AdminPayrollReport.svelte";
import { setLanguage } from "../i18n.js";

const mockState = vi.hoisted(() => ({
  settings: {
    payroll_report_enabled: true,
    payroll_report_recipient: "payroll@example.com",
    payroll_report_day_of_month: 5,
    payroll_report_absence_categories: ["sick"],
    payroll_report_include_assistant_hours: true,
    payroll_report_include_employee_hours: false,
  },
  categories: [
    { id: 1, slug: "sick", name: "Sick", active: true },
    { id: 2, slug: "unpaid", name: "Unpaid", active: true },
    { id: 3, slug: "old_leave", name: "Old leave", active: false },
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
      payroll_report_recipient: "payroll@example.com",
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

  it("renders one checkbox per absence category and preselects the stored ones", async () => {
    component = mount(AdminPayrollReport, { target });
    await settle();

    const checkboxes = [...target.querySelectorAll('input[type="checkbox"]')];
    const sick = checkboxes.find((box) =>
      box.closest("label")?.textContent?.includes("Sick"),
    );
    const unpaid = checkboxes.find((box) =>
      box.closest("label")?.textContent?.includes("Unpaid"),
    );
    expect(sick.checked).toBe(true);
    expect(unpaid.checked).toBe(false);
    // Inactive categories stay selectable but are marked as such.
    expect(target.textContent).toContain("inactive");
  });

  it("saves every configured field including the toggled categories", async () => {
    component = mount(AdminPayrollReport, { target });
    await settle();

    const unpaid = [...target.querySelectorAll('input[type="checkbox"]')].find(
      (box) => box.closest("label")?.textContent?.includes("Unpaid"),
    );
    unpaid.click();
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
    expect(body.payroll_report_recipient).toBe("payroll@example.com");
    expect(body.payroll_report_day_of_month).toBe(5);
    expect(body.payroll_report_absence_categories).toEqual(["sick", "unpaid"]);
    expect(body.payroll_report_include_assistant_hours).toBe(true);
    expect(body.payroll_report_include_employee_hours).toBe(false);
  });

  it("refuses to enable the report without a recipient", async () => {
    mockState.settings = {
      ...mockState.settings,
      payroll_report_recipient: "",
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
