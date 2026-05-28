// Tests for WeekReviewDialog — the approver's dialog for bulk-approving or
// rejecting a submitted week. It shows the employee name, the week label,
// and total hours, then lets the approver approve or reject in one action.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import WeekReviewDialog from "./WeekReviewDialog.svelte";
import { setLanguage } from "../i18n.js";

vi.mock("svelte", async () => {
  return await import("../../node_modules/svelte/src/index-client.js");
});

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

describe("WeekReviewDialog", () => {
  let target;
  let component;
  let originalShowModal;

  const week = {
    user_id: 5,
    week_start: "2026-05-18",
    total_min: 2400, // 40 hours
  };

  const users = [{ id: 5, first_name: "Eve", last_name: "Emp" }];

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    setLanguage("en");
    originalShowModal = HTMLDialogElement.prototype.showModal;
    HTMLDialogElement.prototype.showModal = function () {
      this.setAttribute("open", "");
    };
  });

  afterEach(() => {
    if (component) { unmount(component); component = null; }
    target.remove();
    HTMLDialogElement.prototype.showModal = originalShowModal;
  });

  it("displays the employee's name in the dialog title", async () => {
    // Approvers handle multiple employees; the name makes it clear whose
    // week they are reviewing before approving or rejecting.
    component = mount(WeekReviewDialog, {
      target,
      props: {
        week,
        users,
        busy: false,
        onClose: vi.fn(),
        onApprove: vi.fn(),
        onReject: vi.fn(),
      },
    });
    await settle();
    expect(target.textContent).toContain("Eve Emp");
  });

  it("calls onApprove with the full week object when Approve is clicked", async () => {
    // The caller needs the full week object (including all entry IDs) to
    // build the batch-approve request. Passing only an ID would be insufficient.
    const onApprove = vi.fn();
    component = mount(WeekReviewDialog, {
      target,
      props: {
        week,
        users,
        busy: false,
        onClose: vi.fn(),
        onApprove,
        onReject: vi.fn(),
      },
    });
    await settle();
    const approveBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Approve")
    );
    approveBtn?.click();
    await settle();
    expect(onApprove).toHaveBeenCalledWith(week);
  });

  it("calls onReject with the full week object when Reject is clicked", async () => {
    // Same as approve — the caller needs the week object to build the
    // rejection request with the correct entry IDs.
    const onReject = vi.fn();
    component = mount(WeekReviewDialog, {
      target,
      props: {
        week,
        users,
        busy: false,
        onClose: vi.fn(),
        onApprove: vi.fn(),
        onReject,
      },
    });
    await settle();
    const rejectBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Reject")
    );
    rejectBtn?.click();
    await settle();
    expect(onReject).toHaveBeenCalledWith(week);
  });

  it("disables all action buttons when busy=true to prevent double-submissions", async () => {
    // During an in-flight approve/reject request, the UI must prevent the
    // approver from clicking again — a double submit would create duplicate
    // audit log entries and potentially corrupt approval state.
    component = mount(WeekReviewDialog, {
      target,
      props: {
        week,
        users,
        busy: true,
        onClose: vi.fn(),
        onApprove: vi.fn(),
        onReject: vi.fn(),
      },
    });
    await settle();
    // The Dialog's own header X button is not governed by the busy prop;
    // check only the named action buttons that trigger approve/reject/close API
    // calls — these must be disabled to block a double-submit.
    const actionBtns = [...target.querySelectorAll("button")].filter((b) =>
      b.textContent.includes("Close") ||
      b.textContent.includes("Approve") ||
      b.textContent.includes("Reject")
    );
    expect(actionBtns.length).toBeGreaterThan(0);
    const allDisabled = actionBtns.every((b) => b.disabled);
    expect(allDisabled).toBe(true);
  });

  it("calls onClose when Close is clicked", async () => {
    const onClose = vi.fn();
    component = mount(WeekReviewDialog, {
      target,
      props: {
        week,
        users,
        busy: false,
        onClose,
        onApprove: vi.fn(),
        onReject: vi.fn(),
      },
    });
    await settle();
    const closeBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Close")
    );
    closeBtn?.click();
    await settle();
    expect(onClose).toHaveBeenCalled();
  });
});
