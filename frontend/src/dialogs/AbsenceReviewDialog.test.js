// Tests for AbsenceReviewDialog — the approval/rejection dialog for managers.
// Absences arrive in two flavours: a standard new request ("pending_review")
// and a change to an existing approved absence ("change"). For change requests
// the dialog shows a diff of what changed (e.g. dates extended). Tests verify
// both request types, the employee/type labels, and the action buttons.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import AbsenceReviewDialog from "./AbsenceReviewDialog.svelte";
import { setLanguage, setAbsenceCategoryCache } from "../i18n.js";
import { absenceCategories } from "../stores.js";

vi.mock("svelte", async () => {
  return await import("../../node_modules/svelte/src/index-client.js");
});

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

const baseAbsence = {
  id: 10,
  user_id: 3,
  kind: "vacation",
  status: "pending_review",
  review_type: "new",
  start_date: "2026-07-01",
  end_date: "2026-07-10",
  comment: "",
  created_at: "2026-06-15T09:00:00Z",
  before_data: null,
  after_data: null,
};

const users = [{ id: 3, first_name: "Frank", last_name: "Field" }];

describe("AbsenceReviewDialog", () => {
  let target;
  let component;
  let originalShowModal;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    setLanguage("en");
    const cats = [
      { id: 1, slug: "vacation", name: "Vacation", keeps_work_target: false },
      { id: 2, slug: "sick", name: "Sick", keeps_work_target: false },
    ];
    absenceCategories.set(cats);
    setAbsenceCategoryCache(cats);
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
    // The approver must know who submitted the request before deciding.
    component = mount(AbsenceReviewDialog, {
      target,
      props: {
        absence: baseAbsence,
        users,
        onClose: vi.fn(),
        onApprove: vi.fn(),
        onReject: vi.fn(),
      },
    });
    await settle();
    expect(target.textContent).toContain("Frank Field");
  });

  it("shows the absence type label", async () => {
    // Vacation, sick, and training requests have different quota impacts.
    // The approver needs to see the type to apply the correct company policy.
    component = mount(AbsenceReviewDialog, {
      target,
      props: {
        absence: baseAbsence,
        users,
        onClose: vi.fn(),
        onApprove: vi.fn(),
        onReject: vi.fn(),
      },
    });
    await settle();
    expect(target.textContent).toContain("Vacation");
  });

  it("shows the request comment when present", async () => {
    // A comment gives the approver additional context (e.g. "pre-paid trip").
    component = mount(AbsenceReviewDialog, {
      target,
      props: {
        absence: { ...baseAbsence, comment: "Pre-booked hotel" },
        users,
        onClose: vi.fn(),
        onApprove: vi.fn(),
        onReject: vi.fn(),
      },
    });
    await settle();
    expect(target.textContent).toContain("Pre-booked hotel");
  });

  it("calls onApprove with the absence object when Approve is clicked", async () => {
    // The handler needs the full absence (including ID and status) to call
    // the correct backend endpoint (approve vs. approve-cancellation).
    const onApprove = vi.fn();
    component = mount(AbsenceReviewDialog, {
      target,
      props: {
        absence: baseAbsence,
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
    expect(onApprove).toHaveBeenCalledWith(expect.objectContaining({ id: 10 }));
  });

  it("calls onReject with the absence object when Reject is clicked", async () => {
    // Same reasoning as onApprove — the full absence object is needed to
    // route to the correct rejection endpoint.
    const onReject = vi.fn();
    component = mount(AbsenceReviewDialog, {
      target,
      props: {
        absence: baseAbsence,
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
    expect(onReject).toHaveBeenCalledWith(expect.objectContaining({ id: 10 }));
  });

  it("shows a cancellation chip for cancellation_pending status", async () => {
    // Cancellations are visually distinguished from new requests so the
    // approver understands they are being asked to cancel an approved absence,
    // not to approve a new one.
    component = mount(AbsenceReviewDialog, {
      target,
      props: {
        absence: { ...baseAbsence, status: "cancellation_pending", review_type: "cancellation" },
        users,
        onClose: vi.fn(),
        onApprove: vi.fn(),
        onReject: vi.fn(),
      },
    });
    await settle();
    // Cancellation requests carry a distinct chip style
    const chip = target.querySelector(".zf-chip-cancellation_pending");
    expect(chip).not.toBeNull();
  });
});
