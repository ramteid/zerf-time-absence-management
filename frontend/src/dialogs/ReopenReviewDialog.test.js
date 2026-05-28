// Tests for ReopenReviewDialog — shown to approvers when they review an
// edit request (reopen request) submitted by an employee. The dialog displays
// the week, employee name, request reason, and approve/reject action buttons.
// A submitted week is always handled as a unit; the reopen request grants
// write access to every entry in that week once approved.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import ReopenReviewDialog from "./ReopenReviewDialog.svelte";
import { setLanguage } from "../i18n.js";

vi.mock("svelte", async () => {
  return await import("../../node_modules/svelte/src/index-client.js");
});

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

describe("ReopenReviewDialog", () => {
  let target;
  let component;
  let originalShowModal;

  const item = {
    id: 42,
    user_id: 7,
    week_start: "2026-05-18",
    reason: "Forgot to log Monday",
    created_at: "2026-05-22T08:00:00Z",
  };

  const users = [
    { id: 7, first_name: "Dana", last_name: "Dev" },
  ];

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

  it("shows the requesting employee's name", async () => {
    // Approvers may manage multiple team members; the name helps them identify
    // which employee submitted the request before making a decision.
    component = mount(ReopenReviewDialog, {
      target,
      props: {
        item,
        users,
        onClose: vi.fn(),
        onApprove: vi.fn(),
        onReject: vi.fn(),
      },
    });
    await settle();
    expect(target.textContent).toContain("Dana Dev");
  });

  it("displays the reason the employee provided", async () => {
    // The reason helps approvers decide whether to grant the edit request.
    // A missing reason would force the approver to contact the employee separately.
    component = mount(ReopenReviewDialog, {
      target,
      props: {
        item,
        users,
        onClose: vi.fn(),
        onApprove: vi.fn(),
        onReject: vi.fn(),
      },
    });
    await settle();
    expect(target.textContent).toContain("Forgot to log Monday");
  });

  it("calls onApprove with the request ID when Approve is clicked", async () => {
    // Approving unlocks the week so the employee can edit their time entries.
    // The correct ID must be forwarded so the backend updates the right request.
    const onApprove = vi.fn();
    component = mount(ReopenReviewDialog, {
      target,
      props: {
        item,
        users,
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
    expect(onApprove).toHaveBeenCalledWith(42);
  });

  it("calls onReject with the request ID when Reject is clicked", async () => {
    // Rejection keeps the week locked. The ID must match so the backend
    // rejects the right request and the notification reaches the right employee.
    const onReject = vi.fn();
    component = mount(ReopenReviewDialog, {
      target,
      props: {
        item,
        users,
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
    expect(onReject).toHaveBeenCalledWith(42);
  });

  it("calls onClose when Close is clicked", async () => {
    const onClose = vi.fn();
    component = mount(ReopenReviewDialog, {
      target,
      props: {
        item,
        users,
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
