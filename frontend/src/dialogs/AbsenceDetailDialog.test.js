// Tests for AbsenceDetailDialog — the read-only view an employee sees when
// clicking an absence entry. The dialog shows dates, status, comments, and
// rejection reason. It also conditionally exposes Edit and Cancel buttons
// based on the absence's editable and cancellable flags set by the backend.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import AbsenceDetailDialog from "./AbsenceDetailDialog.svelte";
import { setLanguage } from "../i18n.js";

vi.mock("svelte", async () => {
  return await import("../../node_modules/svelte/src/index-client.js");
});

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

const sampleAbsence = {
  id: 1,
  kind: "vacation",
  start_date: "2026-06-01",
  end_date: "2026-06-05",
  days: 5,
  status: "approved",
  comment: "Summer holiday",
  rejection_reason: null,
  created_at: "2026-05-01T10:00:00Z",
  editable: false,
  cancellable: true,
};

describe("AbsenceDetailDialog", () => {
  let target;
  let component;
  let originalShowModal;

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

  it("displays the from and to dates of the absence", async () => {
    // Employees rely on these fields to verify the dates they submitted
    // match what the approver sees.
    component = mount(AbsenceDetailDialog, {
      target,
      props: {
        absence: sampleAbsence,
        onClose: vi.fn(),
        onEdit: null,
        onCancel: null,
        cancelLabel: "",
      },
    });
    await settle();
    expect(target.textContent).toContain("2026");
  });

  it("shows the employee's comment when present", async () => {
    // Comments provide context to the approver (e.g. "pre-booked hotel").
    // They must be shown to the employee in the detail view for verification.
    component = mount(AbsenceDetailDialog, {
      target,
      props: {
        absence: sampleAbsence,
        onClose: vi.fn(),
        onEdit: null,
        onCancel: null,
        cancelLabel: "",
      },
    });
    await settle();
    expect(target.textContent).toContain("Summer holiday");
  });

  it("shows rejection reason when the absence was rejected", async () => {
    // The rejection reason tells the employee why the request was denied
    // so they can correct their next request or escalate if appropriate.
    const rejectedAbsence = {
      ...sampleAbsence,
      status: "rejected",
      rejection_reason: "Overlaps with mandatory project deadline",
    };
    component = mount(AbsenceDetailDialog, {
      target,
      props: {
        absence: rejectedAbsence,
        onClose: vi.fn(),
        onEdit: null,
        onCancel: null,
        cancelLabel: "",
      },
    });
    await settle();
    expect(target.textContent).toContain("Overlaps with mandatory project deadline");
  });

  it("hides rejection reason when it is null", async () => {
    // For non-rejected absences there is no rejection reason, and the
    // section must be completely absent to keep the dialog clean.
    component = mount(AbsenceDetailDialog, {
      target,
      props: {
        absence: sampleAbsence,
        onClose: vi.fn(),
        onEdit: null,
        onCancel: null,
        cancelLabel: "",
      },
    });
    await settle();
    expect(target.textContent).not.toContain("Rejection reason");
  });

  it("shows a Cancel button when the absence is cancellable and onCancel is provided", async () => {
    // Employees can request cancellation of an already-approved absence
    // (e.g. their holiday plans changed). The button is only shown when
    // the backend has declared the absence cancellable.
    const onCancel = vi.fn();
    component = mount(AbsenceDetailDialog, {
      target,
      props: {
        absence: { ...sampleAbsence, cancellable: true },
        onClose: vi.fn(),
        onEdit: null,
        onCancel,
        cancelLabel: "Request cancellation",
      },
    });
    await settle();
    const cancelBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Request cancellation")
    );
    expect(cancelBtn).not.toBeNull();
    cancelBtn.click();
    await settle();
    expect(onCancel).toHaveBeenCalledWith(expect.objectContaining({ id: 1 }));
  });

  it("hides Cancel button when the absence is not cancellable", async () => {
    // A requested (not yet approved) absence cannot be cancelled — it must
    // be withdrawn directly. Showing a Cancel button here would confuse users.
    component = mount(AbsenceDetailDialog, {
      target,
      props: {
        absence: { ...sampleAbsence, cancellable: false },
        onClose: vi.fn(),
        onEdit: null,
        onCancel: vi.fn(),
        cancelLabel: "Request cancellation",
      },
    });
    await settle();
    const cancelBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Request cancellation")
    );
    expect(cancelBtn).toBeUndefined();
  });

  it("calls onClose when Close is clicked", async () => {
    const onClose = vi.fn();
    component = mount(AbsenceDetailDialog, {
      target,
      props: {
        absence: sampleAbsence,
        onClose,
        onEdit: null,
        onCancel: null,
        cancelLabel: "",
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
