// Tests for ApprovalQueues — the card that shows pending week submissions,
// reopen requests, and absence requests to managers. Tests verify that:
//   - The "Week Approvals" heading renders
//   - A pending count chip appears when there are items to approve
//   - "Approve All" button is shown only when there are pending weeks
//   - Row click calls onOpenWeekDetails with the correct week object
//   - Absence rows show the employee name and absence type

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import ApprovalQueues from "./ApprovalQueues.svelte";
import { setLanguage } from "../../i18n.js";
import { absenceCategories } from "../../stores.js";

vi.mock("svelte", async () => {
  return await import("../../../node_modules/svelte/src/index-client.js");
});

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

const users = [
  { id: 3, first_name: "Bob", last_name: "Emp" },
  { id: 4, first_name: "Carol", last_name: "Dev" },
];

const pendingWeek = {
  key: "3:2026-01-05",
  user_id: 3,
  week_start: "2026-01-05",
  week_end: "2026-01-11",
  total_min: 2400,
  entries: [],
};

const pendingAbsence = {
  id: 10,
  user_id: 4,
  kind: "vacation",
  status: "pending_review",
  review_type: "new",
  start_date: "2026-07-01",
  end_date: "2026-07-05",
  comment: "",
};

describe("ApprovalQueues", () => {
  let target;
  let component;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    setLanguage("en");
    absenceCategories.set([
      { id: 1, slug: "vacation", name: "Vacation", keeps_work_target: false },
      { id: 2, slug: "sick", name: "Sick", keeps_work_target: false },
    ]);
  });

  afterEach(() => {
    if (component) { unmount(component); component = null; }
    target.remove();
  });

  it("renders the Week Approvals heading", async () => {
    component = mount(ApprovalQueues, {
      target,
      props: { pendingWeeks: [], pendingReopens: [], pendingAbsences: [], users },
    });
    await settle();
    expect(target.textContent).toContain("Week Approvals");
  });

  it("shows a pending count chip when there are weeks to approve", async () => {
    // Managers need to know at a glance how many items are waiting without
    // expanding the queue or scrolling through the list.
    component = mount(ApprovalQueues, {
      target,
      props: {
        pendingWeeks: [pendingWeek],
        pendingReopens: [],
        pendingAbsences: [],
        users,
      },
    });
    await settle();
    expect(target.textContent).toContain("pending");
  });

  it("shows 'Approve All' when there are pending weeks", async () => {
    // Batch approval lets managers approve a week of entries in one click
    // when they trust the submitted data without needing to review each entry.
    component = mount(ApprovalQueues, {
      target,
      props: {
        pendingWeeks: [pendingWeek],
        pendingReopens: [],
        pendingAbsences: [],
        users,
      },
    });
    await settle();
    const approveAllBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Approve All")
    );
    expect(approveAllBtn).not.toBeNull();
  });

  it("hides 'Approve All' when there are no pending weeks", async () => {
    // The Approve All button must not appear when there is nothing to approve —
    // clicking it with no pending weeks would be a no-op and confuse managers.
    component = mount(ApprovalQueues, {
      target,
      props: {
        pendingWeeks: [],
        pendingReopens: [],
        pendingAbsences: [],
        users,
      },
    });
    await settle();
    const approveAllBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Approve All")
    );
    expect(approveAllBtn).toBeUndefined();
  });

  it("calls onOpenWeekDetails with the week when a week row is clicked", async () => {
    // The handler needs the full week object to load the detail view and
    // build the review dialog.
    const onOpenWeekDetails = vi.fn();
    component = mount(ApprovalQueues, {
      target,
      props: {
        pendingWeeks: [pendingWeek],
        pendingReopens: [],
        pendingAbsences: [],
        users,
        onOpenWeekDetails,
      },
    });
    await settle();
    const row = target.querySelector("[role='button']");
    row?.click();
    await settle();
    expect(onOpenWeekDetails).toHaveBeenCalledWith(pendingWeek);
  });

  it("renders the employee name for each pending week row", async () => {
    component = mount(ApprovalQueues, {
      target,
      props: {
        pendingWeeks: [pendingWeek],
        pendingReopens: [],
        pendingAbsences: [],
        users,
      },
    });
    await settle();
    expect(target.textContent).toContain("Bob Emp");
  });

  it("renders the Absence Requests section heading", async () => {
    component = mount(ApprovalQueues, {
      target,
      props: { pendingWeeks: [], pendingReopens: [], pendingAbsences: [], users },
    });
    await settle();
    expect(target.textContent).toContain("Absence Requests");
  });

  it("renders a pending absence with the employee's name and type", async () => {
    // Managers review absence requests by name and type so they can apply
    // the correct leave quota and company policy before approving.
    component = mount(ApprovalQueues, {
      target,
      props: {
        pendingWeeks: [],
        pendingReopens: [],
        pendingAbsences: [pendingAbsence],
        users,
      },
    });
    await settle();
    expect(target.textContent).toContain("Carol Dev");
    expect(target.textContent).toContain("Vacation");
  });

  it("renders the comment for a pending absence when present", async () => {
    // The dashboard queue should surface the requester note immediately so
    // approvers do not need to open the detail dialog for basic context.
    component = mount(ApprovalQueues, {
      target,
      props: {
        pendingWeeks: [],
        pendingReopens: [],
        pendingAbsences: [
          {
            ...pendingAbsence,
            comment: "Family appointment in the morning.",
          },
        ],
        users,
      },
    });
    await settle();
    expect(target.textContent).toContain("Family appointment in the morning.");
  });
});
