// Tests for FlextimeAccountDialog — the only place a flextime balance can be
// changed by hand. Key rules:
//   - Bookings are dated from the contract start onwards, the future included
//   - Hours typed by the admin are converted to signed minutes
//   - Entries are cancelled, never deleted: the opposite amount is booked and
//     both rows stay on the record
//   - Non-admins get a read-only view: no form, no cancel buttons
//   - A user without a flextime account (assistant, pure admin) gets an
//     explanation instead of a balance

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import FlextimeAccountDialog from "./FlextimeAccountDialog.svelte";
import { settings } from "../stores.js";
import { setLanguage } from "../i18n.js";

const getAccountMock = vi.hoisted(() => vi.fn());
const createMock = vi.hoisted(() => vi.fn());
const reverseMock = vi.hoisted(() => vi.fn());

vi.mock("svelte", async () => {
  return await import("../../node_modules/svelte/src/index-client.js");
});

vi.mock("../lib/api/usersApi.js", () => ({
  getFlextimeAccount: getAccountMock,
  createFlextimeAdjustment: createMock,
  reverseFlextimeAdjustment: reverseMock,
}));

vi.mock("../confirm.js", () => ({
  confirmDialog: vi.fn().mockResolvedValue(true),
}));

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

async function waitForText(target, text, timeout = 5000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (target.textContent?.includes(text)) return;
    await new Promise((r) => setTimeout(r, 25));
  }
  throw new Error(`Text not found: "${text}"`);
}

function account(overrides = {}) {
  return {
    user_id: 7,
    user_name: "Grace Green",
    has_flextime_account: true,
    start_date: "2024-01-01",
    balance_min: 630,
    balance_as_of: "2026-08-16",
    adjustments: [
      {
        id: 11,
        user_id: 7,
        effective_date: "2024-01-01",
        minutes: 600,
        kind: "opening_balance",
        reason: null,
        created_by: 1,
        created_by_name: "Ada Admin",
        reverses_id: null,
        reversed: false,
        created_at: "2024-01-01T08:00:00Z",
      },
      {
        id: 12,
        user_id: 7,
        effective_date: "2026-06-30",
        minutes: -120,
        kind: "correction",
        reason: "Overtime payout",
        created_by: 1,
        created_by_name: "Ada Admin",
        reverses_id: null,
        reversed: false,
        created_at: "2026-06-30T09:00:00Z",
      },
    ],
    ...overrides,
  };
}

describe("FlextimeAccountDialog", () => {
  let target;
  let component;
  let originalShowModal;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    setLanguage("en");
    settings.set({ ui_language: "en", time_format: "24h", timezone: "UTC" });
    getAccountMock.mockReset();
    createMock.mockReset();
    reverseMock.mockReset();
    getAccountMock.mockResolvedValue(account());
    createMock.mockResolvedValue({ id: 13 });
    reverseMock.mockResolvedValue({ id: 14 });
    originalShowModal = HTMLDialogElement.prototype.showModal;
    HTMLDialogElement.prototype.showModal = function () {
      this.setAttribute("open", "");
    };
  });

  afterEach(() => {
    if (component) {
      unmount(component);
      component = null;
    }
    target.remove();
    HTMLDialogElement.prototype.showModal = originalShowModal;
  });

  it("lists every booking behind the balance", async () => {
    component = mount(FlextimeAccountDialog, {
      target,
      props: { userId: 7, onClose: vi.fn(), canEdit: true },
    });
    await waitForText(target, "Grace Green");
    await settle();

    // Balance and both bookings, each with its sign.
    expect(target.textContent).toContain("+10:30");
    expect(target.textContent).toContain("+10:00");
    expect(target.textContent).toContain("-2:00");
    expect(target.textContent).toContain("Overtime payout");
    expect(target.textContent).toContain("Ada Admin");
  });

  it("converts typed hours into signed minutes when booking", async () => {
    component = mount(FlextimeAccountDialog, {
      target,
      props: { userId: 7, onClose: vi.fn(), canEdit: true },
    });
    await waitForText(target, "Grace Green");
    await settle();

    const hours = target.querySelector("#flextime-adjustment-hours");
    hours.value = "-2.5";
    hours.dispatchEvent(new Event("input", { bubbles: true }));
    const note = target.querySelector("#flextime-adjustment-reason");
    note.value = "  Payout  ";
    note.dispatchEvent(new Event("input", { bubbles: true }));
    await settle();

    const addButton = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Add entry"),
    );
    addButton.click();
    await settle();
    await settle();

    expect(createMock).toHaveBeenCalledTimes(1);
    const [userId, payload] = createMock.mock.calls[0];
    expect(userId).toBe(7);
    expect(payload.minutes).toBe(-150);
    expect(payload.reason).toBe("Payout");
  });

  it("refuses to book nothing", async () => {
    component = mount(FlextimeAccountDialog, {
      target,
      props: { userId: 7, onClose: vi.fn(), canEdit: true },
    });
    await waitForText(target, "Grace Green");
    await settle();

    const addButton = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Add entry"),
    );
    addButton.click();
    await settle();

    expect(createMock).not.toHaveBeenCalled();
    expect(target.textContent).toContain(
      "Enter the number of hours to add or subtract",
    );
  });

  it("is read-only without edit rights", async () => {
    component = mount(FlextimeAccountDialog, {
      target,
      props: { userId: 7, onClose: vi.fn(), canEdit: false },
    });
    await waitForText(target, "Grace Green");
    await settle();

    // The bookings are still listed — understanding your own balance is the
    // reason the view is readable at all — but nothing can be changed.
    expect(target.textContent).toContain("Overtime payout");
    expect(target.querySelector("#flextime-adjustment-hours")).toBeNull();
    expect(target.querySelector("#flextime-adjustment-date")).toBeNull();
    expect(
      [...target.querySelectorAll("button")].some(
        (b) => b.title === "Cancel entry",
      ),
    ).toBe(false);
  });

  it("explains itself for a user without a flextime account", async () => {
    getAccountMock.mockResolvedValue(
      account({
        has_flextime_account: false,
        balance_min: null,
        balance_as_of: null,
        adjustments: [],
      }),
    );
    component = mount(FlextimeAccountDialog, {
      target,
      props: { userId: 7, onClose: vi.fn(), canEdit: true },
    });
    await waitForText(target, "no flextime account");
    await settle();

    expect(target.querySelector("#flextime-adjustment-hours")).toBeNull();
  });

  it("never defaults to a date before the contract starts", async () => {
    // The ledger does not exist before the contract start, so for someone who
    // has not started yet the default has to be their first day rather than
    // today, which the backend would reject.
    getAccountMock.mockResolvedValue(account({ start_date: "2099-01-01" }));
    component = mount(FlextimeAccountDialog, {
      target,
      props: { userId: 7, onClose: vi.fn(), canEdit: true },
    });
    await waitForText(target, "Grace Green");
    await settle();

    expect(target.querySelector("#flextime-adjustment-date").value).toBe(
      "2099-01-01",
    );
  });

  it("cancels an entry instead of deleting it", async () => {
    // Deleting would move every balance reported since that date with nothing
    // left to explain it — the exact problem this account exists to remove.
    component = mount(FlextimeAccountDialog, {
      target,
      props: { userId: 7, onClose: vi.fn(), canEdit: true },
    });
    await waitForText(target, "Grace Green");
    await settle();

    const cancelButton = [...target.querySelectorAll("button")].find(
      (b) => b.title === "Cancel entry",
    );
    expect(cancelButton).toBeTruthy();
    cancelButton.click();
    await settle();
    await settle();

    expect(reverseMock).toHaveBeenCalledWith(11);
  });

  it("offers no cancel button on an entry that is already cancelled", async () => {
    getAccountMock.mockResolvedValue(
      account({
        adjustments: [
          {
            id: 11,
            user_id: 7,
            effective_date: "2026-06-30",
            minutes: -120,
            kind: "correction",
            reason: "Typo",
            created_by: 1,
            created_by_name: "Ada Admin",
            reverses_id: null,
            reversed: true,
            created_at: "2026-06-30T09:00:00Z",
          },
          {
            id: 12,
            user_id: 7,
            effective_date: "2026-06-30",
            minutes: 120,
            kind: "correction",
            reason: null,
            created_by: 1,
            created_by_name: "Ada Admin",
            reverses_id: 11,
            reversed: false,
            created_at: "2026-06-30T09:05:00Z",
          },
        ],
      }),
    );
    component = mount(FlextimeAccountDialog, {
      target,
      props: { userId: 7, onClose: vi.fn(), canEdit: true },
    });
    await waitForText(target, "Grace Green");
    await settle();

    // Both rows stay visible — the mistake and its cancellation.
    expect(target.textContent).toContain("Typo");
    expect(target.textContent).toContain("Cancelled");
    expect(target.textContent).toContain("Cancellation");
    expect(
      [...target.querySelectorAll("button")].filter(
        (b) => b.title === "Cancel entry",
      ),
    ).toHaveLength(0);
  });

  it("reports back that something changed after a booking", async () => {
    const onClose = vi.fn();
    component = mount(FlextimeAccountDialog, {
      target,
      props: { userId: 7, onClose, canEdit: true },
    });
    await waitForText(target, "Grace Green");
    await settle();

    const hours = target.querySelector("#flextime-adjustment-hours");
    hours.value = "1";
    hours.dispatchEvent(new Event("input", { bubbles: true }));
    await settle();
    [...target.querySelectorAll("button")]
      .find((b) => b.textContent.includes("Add entry"))
      .click();
    await settle();
    await settle();

    [...target.querySelectorAll("button")]
      .find((b) => b.textContent.trim() === "Close")
      .click();
    await settle();

    expect(onClose).toHaveBeenCalledWith(true);
  });
});
