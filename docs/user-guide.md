# Zerf User Guide

This guide explains how to use Zerf in daily work and how core workflow logic behaves.

Use this document if you are:

- an employee who needs a quick start,
- an approver who needs to review requests,
- an admin who needs to understand role and process behavior,
- anyone who wants clear answers about status logic, balances, and edge cases.

## Table of contents

- [Quick start](#quick-start)
  - [1. First login](#1-first-login)
  - [2. Your first work week](#2-your-first-work-week)
  - [3. If you need to correct submitted data](#3-if-you-need-to-correct-submitted-data)
- [Core concept: crediting vs. non-crediting entries](#core-concept-crediting-vs-non-crediting-entries)
  - [Key insight: workflow vs. work-math](#key-insight-workflow-vs-work-math)
  - [Practical examples](#practical-examples)
- [Roles and approval model](#roles-and-approval-model)
  - [Role colors and list order](#role-colors-and-list-order)
- [Timezone and date behavior](#timezone-and-date-behavior)
- [Time entry workflow](#time-entry-workflow)
  - [Status lifecycle](#status-lifecycle)
  - [Weekly process](#weekly-process)
  - [Time-entry summary tiles](#time-entry-summary-tiles)
  - [Understanding crediting vs. non-crediting entries](#understanding-crediting-vs-non-crediting-entries)
  - [Important workflow rule](#important-workflow-rule)
  - [Approval permissions and scope](#approval-permissions-and-scope)
- [Changes after submission](#changes-after-submission)
  - [Request edit (week level)](#request-edit-week-level)
- [Absence workflow](#absence-workflow)
  - [Status lifecycle](#status-lifecycle-1)
  - [Auto-approval](#auto-approval)
  - [Overlap rules](#overlap-rules)
- [Flextime logic](#flextime-logic)
  - [How daily targets are calculated](#how-daily-targets-are-calculated)
  - [What counts toward flextime actuals](#what-counts-toward-flextime-actuals)
  - [Automatic break deduction](#automatic-break-deduction)
- [Submission status indicator](#submission-status-indicator)
  - [How completeness is determined](#how-completeness-is-determined)
  - [Important: non-crediting entries affect completeness](#important-non-crediting-entries-affect-completeness)
- [Leave accounts and carryover logic](#leave-accounts-and-carryover-logic)
  - [Balance cards](#balance-cards)
  - [Entitlement, start year, and carryover](#entitlement-start-year-and-carryover)
- [Notifications](#notifications)
  - [Employee receives notifications when](#employee-receives-notifications-when)
  - [Approver receives notifications when](#approver-receives-notifications-when)
  - [Exception: auto-approved submissions and reopen requests are silent](#exception-auto-approved-submissions-and-reopen-requests-are-silent)
  - [Who gets notified](#who-gets-notified)
  - [Important: non-crediting entries trigger reminders too](#important-non-crediting-entries-trigger-reminders-too)
  - [Monthly submission reminder](#monthly-submission-reminder)
  - [Weekly approval reminder](#weekly-approval-reminder)
  - [Reminder toggles (admin)](#reminder-toggles-admin)
  - [System error notifications (admin)](#system-error-notifications-admin)
  - [Notification timestamp display](#notification-timestamp-display)
- [Important edge case: sick leave with existing time entries](#important-edge-case-sick-leave-with-existing-time-entries)
- [Approval structure examples](#approval-structure-examples)
  - [Role organigram](#role-organigram)
  - [Example approval flow](#example-approval-flow)
  - [What explicit assignment means](#what-explicit-assignment-means)
- [Reporting behavior (important)](#reporting-behavior-important)
  - [Reports page layout](#reports-page-layout)
  - [Month and overtime/flextime math](#month-and-overtimeflextime-math)
  - [Category breakdown reports](#category-breakdown-reports)
  - [Team report scope](#team-report-scope)
- [Admin checklist for a correct setup](#admin-checklist-for-a-correct-setup)
- [FAQ](#faq)
  - [Why can my approver not see my entries?](#why-can-my-approver-not-see-my-entries)
  - [Why was my absence rejected even though dates were valid?](#why-was-my-absence-rejected-even-though-dates-were-valid)
  - [Why does my flextime increase on a sick day?](#why-does-my-flextime-increase-on-a-sick-day)
  - [Why does submission status show missing weeks even though current week is in progress?](#why-does-submission-status-show-missing-weeks-even-though-current-week-is-in-progress)
  - [Why don't the hours I booked today change my flextime balance?](#why-dont-the-hours-i-booked-today-change-my-flextime-balance)
- [Employee workflow reference](#employee-workflow-reference)
  - [Recording time entries](#recording-time-entries)
  - [Submitting a week](#submitting-a-week)
  - [Requesting a week reopen](#requesting-a-week-reopen)
  - [Absences: creating](#absences-creating)
  - [Absences: editing a pending absence](#absences-editing-a-pending-absence)
  - [Absences: cancelling](#absences-cancelling)
  - [Leave accounts](#leave-accounts)
- [Team lead workflow reference](#team-lead-workflow-reference)
  - [Scope of lead authority](#scope-of-lead-authority)
  - [Reviewing time entries (week level)](#reviewing-time-entries-week-level)
  - [Reviewing an absence](#reviewing-an-absence)
  - [Reviewing an absence cancellation](#reviewing-an-absence-cancellation)
  - [Reviewing a reopen request](#reviewing-a-reopen-request)
  - [Team settings: reopen policy](#team-settings-reopen-policy)
  - [Team settings: submission policy](#team-settings-submission-policy)
  - [Viewing team reports](#viewing-team-reports)
  - [Scoped assistant user management (optional)](#scoped-assistant-user-management-optional)
- [Admin workflow reference](#admin-workflow-reference)
  - [Reading the audit log](#reading-the-audit-log)
  - [Reading the system log](#reading-the-system-log)
  - [Creating a user](#creating-a-user)
  - [Updating a user](#updating-a-user)
  - [Archiving a user](#archiving-a-user)
  - [Restoring an archived user](#restoring-an-archived-user)
  - [Deleting a user](#deleting-a-user)
  - [Resetting a password](#resetting-a-password)
  - [Managing approver assignments](#managing-approver-assignments)
  - [Direct correction of submitted or approved entries](#direct-correction-of-submitted-or-approved-entries)
  - [Managing leave accounts](#managing-leave-accounts)
  - [Revoking an approved absence](#revoking-an-approved-absence)
  - [System settings](#system-settings)
  - [Nextcloud Upload](#nextcloud-upload)
  - [Payroll Report](#payroll-report)
  - [Managing categories](#managing-categories)
  - [Managing holidays](#managing-holidays)
  - [Backup and restore](#backup-and-restore)
- [Status transition reference](#status-transition-reference)
  - [Time entry statuses](#time-entry-statuses)
  - [Absence statuses](#absence-statuses)
  - [Reopen request statuses](#reopen-request-statuses)
- [Security and access control](#security-and-access-control)
  - [Authentication](#authentication)
  - [Temporary passwords and forced password change](#temporary-passwords-and-forced-password-change)
  - [Role-based access control](#role-based-access-control)
  - [Pure-admin mode (tracks_time=false)](#pure-admin-mode-tracks_timefalse)
  - [Session invalidation](#session-invalidation)
  - [Audit trail](#audit-trail)
  - [Input validation and DoS prevention](#input-validation-and-dos-prevention)
  - [Information disclosure prevention](#information-disclosure-prevention)

## Quick start

### 1. First login

1. Open your Zerf URL and sign in with your account.
2. Check your profile settings (name, language, weekly hours).
3. Confirm that an approver is assigned if you are not an admin.

### 2. Your first work week

1. Create daily time entries as `Draft`.
2. Add absences if needed (vacation, sick leave, training, etc.).
3. At end of week, use `Submit Week`.
4. Track approval results and notifications.

### 3. If you need to correct submitted data

- Click `Request edit` on the affected week. Once your team lead approves
  (or auto-approval is enabled), every entry in that week becomes editable
  again.
- A submitted week is always handled as a single unit — individual entries
  inside it cannot be modified separately.

## Core concept: crediting vs. non-crediting entries

Zerf tracks two types of work time entries, and understanding the difference will help you use the system more effectively.

Every work category (like "Project work", "Team meeting", etc.) is configured as either **crediting** or **non-crediting**. This determines whether the hours count toward your work targets and flextime balance.

| Type | Examples | Counts toward targets? | Counts toward flextime? | Requires approval? |
| --- | --- | --- | --- | --- |
| **Crediting** | Project work, Client support, Sales | ✓ Yes | ✓ Yes | ✓ Yes |
| **Non-crediting** | Meetings, Training, Internal admin | ✗ No | ✗ No | ✓ Yes (same as all entries) |

### Key insight: workflow vs. work-math

- **Workflow** (submission, approval, reminders): All entries participate equally, whether crediting or non-crediting.
- **Work-math** (flextime, targets, reports): Only crediting entries count.

This means:

- You must submit both types of entries. Non-crediting entries do not skip the approval workflow.
- Your weekly completeness status includes both types. If you have unsubmitted non-crediting entries, your week is incomplete.
- Only crediting entry hours affect your flextime calculation and whether you hit your daily/monthly targets.
- Non-crediting entries are recorded for transparency and audit, but they do not impact your work metrics.

### Practical examples

**Example 1: Completeness check**
- You have 8h crediting work all week (submitted/approved).
- You have 2h team meetings (non-crediting, still in draft).
- Your week status: **Incomplete** — you must submit the meetings too.
- Once you submit them, your week is **Complete** and ready for reporting.

**Example 2: Flextime calculation**
- Your daily target: 8 hours
- You log: 6h crediting work + 2h training (non-crediting)
- Flextime delta: 6 − 8 = **−2 hours** (only the 6h crediting work counts)
- The 2h training is recorded but does not affect your flextime.

**Example 3: Reopen request**
- Your week has 8h crediting work and 2h meetings (both submitted).
- You request to reopen the week.
- Result: the reopenable entries in the week are reset to draft and can be edited again.

If you are unsure which categories in your organization are crediting, ask your admin or check the category list in the Settings. Inactive categories remain visible to admins for maintenance, but they are hidden from normal time-entry forms.

## Roles and approval model

Zerf uses explicit approver assignments. Approvals and notifications are not
inferred from role alone.

- Employee: records time and absences, submits weeks, requests changes.
- Assistant ("Aushilfe"): records time and absences like an employee, but has
  no working-hours quota — no fixed weekly/daily target and therefore no
  flextime account. Assistants are simply paid for the hours they are present
  and work. Their weekly hours are set to `0` by convention, but the role, not
  the zero, is what defines them: the "no target, no flextime" behaviour is
  strictly role-based and is never inferred from weekly hours being zero.
- A non-assistant with weekly hours set to `0` is a non-booking user: approval
  logic still applies to anything they do book, but they are exempt from
  monthly submission reminders and from week-completeness requirements (the
  Submissions tile, team report, and monthly PDF upload never flag them for
  "weeks missing").
- Approver: a user who has been explicitly assigned to another user and is
	active.
- Admin: manages users, categories, holidays, settings, and can also be an
	approver if explicitly assigned.

Important rules:

- Every approval workflow is driven by explicit assignment.
- A user can have multiple approvers. If more than one active approver is
	assigned, all of them are treated as valid recipients and reviewers for that
	user's requests.
- Admin users do not automatically receive notifications just because they are
	admins. They only receive approval notifications when they are explicitly
	assigned.
- Non-admin approvers cannot act on admin users. Admin-subject requests are
	handled by admins only.
- Only active approvers are considered. Inactive users are ignored for routing
	and review.

This means the assignment list is the single source of truth for who gets asked
to review a request.

### Role colors and list order

To make roles easy to recognise at a glance, Zerf colors each user's avatar
(the circle with their initials) by role, and shows the same color everywhere
that user appears (sidebar, account page, user lists, approval queues,
dashboards). The colors are pastel and consistent:

| Role | Avatar color |
|------|--------------|
| Team lead | Blue |
| Employee | Green |
| Assistant | Light green |
| Admin | Red |

Wherever a list of users is shown — the admin Users tab, Team Settings, the
approver pickers when creating/archiving/restoring a user, the report
employee dropdowns, and the dashboard "Who is absent" list — users are grouped
by role in the order **team leads, employees, assistants, admins**, and sorted
alphabetically by name within each group. The combined "all employees"
timesheet PDF export (see [Viewing team reports](#viewing-team-reports)) orders
its sections the same way.

One deliberate exception: on the scoped **Users** tab a non-admin team lead
sees (the optional assistant-management view), colleagues who are not the
lead's assistants have their role hidden by the server, so that list stays in
plain alphabetical order and those avatars use the neutral default color.

An admin can optionally grant non-admin team leads a narrow, additional
capability: creating and managing "Assistant" users assigned to them. This is
off by default and controlled by a single setting; see [Scoped assistant user
management (optional)](#scoped-assistant-user-management-optional) and [System
settings](#system-settings).

## Timezone and date behavior

Zerf uses one configurable application timezone for all business date logic.

What this means in practice:

- Admins can set the app timezone in settings (Settings → General, IANA zone, for example
	`Europe/Berlin`).
- "Today", current year/month boundaries, reminder scheduling dates, and
	date-based workflow checks are calculated in the configured app timezone.
- User-facing dates and timestamps in UI, emails, and notifications are
	formatted in the configured app timezone.
- End users do not need to configure a personal business timezone for workflow
	behavior; workflow date logic is consistent system-wide.

This prevents "wrong day" edge cases around midnight and daylight-saving
changes when users and server run in different timezones.

## Time entry workflow

### Status lifecycle

| Status | Meaning |
| --- | --- |
| Draft | Created by employee. Not yet in review. |
| Submitted | Week was submitted. Approvers can review. |
| Approved | Entry accepted. Included in reports and flextime logic. |
| Rejected | Entry rejected. It stays visible as history; an overlapping approved correction closes it for completeness and reopen checks. |

Users with submission auto-approval enabled skip `Submitted` entirely: their
entries go directly from `Draft` to `Approved` on submit (see [Team settings:
submission policy](#team-settings-submission-policy)).

### Weekly process

1. Create daily draft entries.
2. Submit the full week with `Submit Week`.
3. Approver accepts or rejects the week in batch.
4. Approved entries remain valid unless the whole week is reopened via a new edit request.

A submitted week is treated atomically. Individual entries inside a submitted,
approved, or rejected week cannot be clicked or edited any more — the only way
to correct them is to reopen the whole week (see "Changes after submission").

### Time-entry summary tiles

In the weekly Time Entry view, the first summary tile always shows recorded
crediting hours for the current week (rejected entries are excluded).

- Display format: decimal hour values always use two decimal places (for example `6.00h of 8.00h target`).
- Color logic:
  - red when logged hours are below the weekly target,
  - green when logged hours are equal to or above the weekly target.
- The `Status` tile remains workflow-only and uses the same value font size as
  the logged-hours tile for consistent readability.

### Understanding crediting vs. non-crediting entries

Each work category in Zerf is configured as either **crediting** or
**non-crediting**.

**Crediting entries** (for example project work, client support):

- count toward daily and monthly targets,
- affect flextime balances.

**Non-crediting entries** (for example meetings, training, internal admin):

- follow the same submission and approval workflow,
- do not change flextime or target-hour math.

### Important workflow rule

All entries participate in workflow equally:

- submission,
- approval/rejection,
- completeness checks,
- reminders,
- reopen workflows.

### Approval permissions and scope

- Non-admin approvers can review only users explicitly assigned to them.
- Non-admin approvers cannot manage admin-subject workflow items.
- Admins can review all users.

The same scope rule is applied across time entries, absences, reopen requests,
and lead-scoped team views.

## Changes after submission

A submitted week is locked at the week level. There is no per-entry edit
workflow: once you have submitted a week, clicking an individual time entry
does nothing. The only way to make corrections is to reopen the whole week.

### Request edit (week level)

- Use this whenever a submitted, approved, or rejected entry needs to be
  corrected — whether it is one entry or several.
- An approved reopen resets submitted and approved entries in that week back to
  `Draft`. Rejected entries are reset when they still have no submitted or
  approved replacement on the same day.
- Reopened entries become editable; once the corrections are done, submit the
  week again.
- If all reopenable entries in the week are still waiting for approval as
  `Submitted`, `Request edit` reopens it immediately and removes the submitted
  week from the approval queue. No separate edit approval is shown to approvers
  in parallel with the original submission.
- If a week has no submitted, approved, or rejected entries, the edit request
  is rejected with a message that the week has no submitted, approved, or
  rejected entries.
- Reopen requests can be pending review or auto-approved, depending on the
  requester's configuration (see Settings → Team Settings → "Auto-approve edit requests").

## Absence workflow

### Status lifecycle

| Status | Meaning |
| --- | --- |
| Requested | Sent by employee, waiting for decision. |
| Approved | Accepted by approver. Covered workdays have target hours 0. |
| Rejected | Declined by approver. |
| Cancellation pending | Employee asked to cancel an approved absence. |
| Cancelled | Approved absence was cancelled. Daily target returns to normal rules. |

### Auto-approval

- Absence categories marked **Auto-approve past dates** (e.g. sick leave) with a start date on or before today are auto-approved.
  Your approvers receive an informational notice, in-app and by email (not an action request).
- Other absence types require explicit approval.

### Overlap rules

- A request must include at least one effective workday (not weekend-only, not holiday-only).
- An absence request can span at most 365 days (i.e., end_date - start_date ≤ 365).
- Requesting an absence that overlaps days with existing time entries is allowed; however, the approver will see the conflict and the approval will be blocked until the time entries are removed or rejected.
- Once an absence is in *requested* status, new time entries on the covered days are blocked (to prevent the conflict from worsening while approval is pending).
- If an approved absence covers a day that already has time entries, those entries remain and still count as worked time.

Review and privacy behavior:

- Non-admin approvers can approve/reject only direct-report absences for
	non-admin users.
- Admin-subject absences are handled by admins.
- Calendar visibility is strictly role-scoped:
	- Employees and assistants see only their own absences and time entries.
	- Team leads see their own data plus the absences and time entries of
		every user who has them assigned as approver (their direct reports,
		excluding admin subjects). For direct reports' time entries, the
		person's name is shown in the event detail.
	- Admins see all users' data regardless of approver assignments.
- **Calendar visibility is governed solely by the requester's scope**:
	admins see all absences, leads see their own plus their direct reports',
	and employees see only their own. There is no per-category carve-out —
	a category is either visible because the viewer's scope covers the
	owner, or it is not visible at all.
- Comments carry no separate restriction: whoever's scope covers the
	absence owner (the owner themselves, their assigned leads, and any
	admin) sees the comment along with the rest of the entry. There is no
	redacted or masked view — see [Information disclosure
	prevention](#information-disclosure-prevention).

Vacations and sick leave are checked against the employee's own work schedule.
A one-day request on a public holiday or on a non-working weekday does not
count as a valid absence day.

## Flextime logic

Flextime (positive or negative balance) is calculated as:

**Flextime = Actual work hours − Daily targets**

Only **crediting entries** count as actual work hours in this calculation. Non-crediting entries are recorded and approved like all others, but they do not contribute to your flextime.

**The flextime balance is calculated up to and including yesterday — today's
hours are not yet counted toward the balance.** This applies everywhere the
balance is shown (dashboard, reports, team overview, balance chart, exports)
and regardless of whether the days are inside a submitted or approved week.
Today's hours still appear in your time entries, the monthly logged-hours
tile, and category breakdowns; they only stop short of contributing to the
running balance until tomorrow.

Users with role `assistant` do not have a flextime account. This behavior is
role-based (not inferred from weekly hours). For assistants, flextime and
overtime reports return no rows and submission completeness for past weeks is
treated as complete.

### How daily targets are calculated

Daily target is the number of hours you are expected to work on a given day.

Daily target hours are `0` when:

- Day is a weekend (for your configured work schedule),
- Day is a public holiday,
- Day is covered by an approved absence (vacation, sick leave, training, etc.),
- Day is before your start date,
- Day is in the future.

Absences from categories with cost type `flextime` (e.g. flextime reduction) are the exception: they follow the absence workflow and block normal time entry creation on those days, but the daily work target is not removed. This lets the days reduce your flextime balance intentionally. To prevent the balance from going below the configured minimum (default 0 minutes; admin can override via the `flextime_min_balance_min` setting), the balance is checked TWICE: when you submit the request AND when the approver approves it. The check accounts for any other already-pending/approved flextime-cost absences you have so multiple requests that each individually fit cannot together breach the floor, and the approver's re-check catches the case where you spent balance between request and approval.

Otherwise, target is calculated as:

**Daily target = (Weekly hours ÷ Workdays per week) × (1 day)**

Example: If you work 40 hours per week over 5 days, your daily target is 8 hours.

### What counts toward flextime actuals

- **Approved crediting entries:** hours count fully.
- **Submitted crediting entries:** hours do NOT count in the official flextime actuals, but they are included in the Overtime overview tile balance as a projected total (see note below).
- **Draft crediting entries:** hours do NOT count.
- **Non-crediting entries (all statuses):** hours do NOT count, regardless of approval status.

**Overtime overview tile:** The balance shown in the `Overtime overview` tile on the dashboard includes both approved and submitted (pending approval) crediting hours filed up to and including yesterday. This gives you a projected total reflecting everything you have filed so far (today's hours are still excluded — see above). If there are no pending approvals, the displayed value equals the official approved balance. When submitted hours are pending, the sub-text shows the approved-only balance for reference.

Example flextime scenario:

- Your daily target: 8 hours
- Monday approved work entries (crediting): 7 hours → Flextime delta: −1 hour
- Monday team meeting (non-crediting): 1 hour → Does NOT affect flextime
- Monday total actual hours for flextime: 7 hours (only crediting counted)
- Your Monday flextime result: 7 − 8 = −1 hour

If your team meeting were crediting instead, the result would be: (7+1) − 8 = 0 hours flextime.

### Automatic break deduction

When the feature is enabled in Settings → General, Zerf silently deducts a configured number of break minutes from each day's credited work when consecutive work exceeds a configured threshold. The threshold is exclusive: exactly 6 hours of consecutive work does not trigger a 6-hour rule, only 6 hours and 1 minute or more does (matching German labor law, ArbZG §4, which requires a break only for work of *more than* six hours).

**How continuity is determined:**

- Crediting time entries are examined per day only. Work time does not carry over across midnight.
- Two entries are treated as one continuous block when one ends at the exact minute the next begins (zero gap). Even a one-minute gap between entries breaks continuity into separate blocks.
- Overlapping entries are merged into one block.

**Deduction logic:**

- Up to two break tiers can be configured. For each continuous block, only the **highest applicable tier** fires — the tiers are **not cumulative**.
  - Example: tier 1 = 6 h → 30 min; tier 2 = 9 h → 45 min. A 10-hour block deducts 45 min total, not 75 min.
- If a day has two separate long blocks (morning and afternoon each exceeding the threshold), each block is evaluated independently and triggers its own deduction.
- The deduction is applied to approved crediting time. It reduces credited hours in month reports, overtime, and the flextime balance.

**What is not affected:**

- The deduction is not labeled or shown in reports, team overviews, or CSV exports. It reduces the total silently.
- For the official flextime balance and reports, only approved entries are used in the deduction calculation. Draft and submitted entries do not affect the flextime account.
- Non-crediting entries are not considered when computing consecutive blocks.

**Visual indicator on the time tracking page:**

The time tracking page applies the break deduction as a preview for all non-rejected entries (including drafts and submitted entries) so you can see the impact before approval. The daily total shown next to each day already includes this preview deduction. This preview matches the deduction that will be applied to the flextime balance once entries are approved.

When a break is triggered, the entry block where the threshold is crossed displays a horizontal marker. Its vertical position reflects the exact moment within that entry when the threshold is exceeded, and its height is proportional to the deduction duration relative to the entry's length.

Example: threshold 6 hours, deduction 30 minutes. An employee books 3 hours of core work followed immediately by 4 hours of training (7 hours total, one continuous block). The threshold is crossed during the training block, 3 hours into it (6 total hours exceeded). The marker appears at three-quarters from the top of the training entry block, and its height corresponds to 30 minutes of the 4-hour entry (about 12.5 % of the block height).

**Configuration (Settings → General):**

| Setting | Description |
| --- | --- |
| Enable automatic break deduction | Enables or disables the feature. When disabled, all stored values are cleared. |
| Break threshold (hours) | Tier-1 minimum consecutive crediting work duration that triggers a break (must be greater than 0, up to 24 h). A block must strictly exceed this duration — exactly matching it does not trigger a deduction. |
| Break deduction (minutes) | Tier-1 total minutes deducted once the threshold is exceeded (1–480 min). |
| Second threshold (hours) | Optional tier-2 threshold. Must be greater than tier-1. Once a block's duration exceeds this threshold, the tier-2 deduction replaces tier-1. |
| Second deduction (minutes) | Tier-2 total minutes deducted (1–480 min). This is the total, not additional — e.g. configure 45 min here, not 15 min, to achieve a 45-minute break at the tier-2 threshold. |

## Submission status indicator

The `Submissions` tile shows whether all required past weeks have been submitted and approved.

- **Scope:** from your start date up to and including the last complete week.
- **Current week is excluded** from this check (it is still ongoing).

### How completeness is determined

Completeness is checked per fully elapsed week against your configured
`workdays per week` value. It does not matter whether you reached your weekly
target hours; what counts is that the required number of days is covered.

Important: Zerf does **not** pin you to fixed weekdays (for example, not
automatically "Mon-Thu" for 4-day schedules). For 1-5 day schedules, days are
treated as flexible within Monday-Friday.

A week is considered **complete** when:

- No entry anywhere in the week is still in draft state, and no rejected entry
  remains without an overlapping approved correction, **and**
- The week has enough covered days for your configured day quota
  (`workdays per week`). A day is covered when it has at least one submitted or
  approved entry (crediting or non-crediting), or is excused by an approved,
  cancellation-pending, or requested absence, a public holiday, or falling
  before your contract start date.
  (A week with no entries at all is complete when enough days are excused,
  for example a full-vacation week.)

For users with role `assistant`, past-week completeness is always treated as
complete.

A week is considered **incomplete** when:

- Any entry anywhere in the week is still in draft state, or a rejected entry
  has not yet been closed by an overlapping approved correction,
  **or**
- Covered days in the week are fewer than your configured weekly day quota.
  Submitting only some days can still be incomplete if the quota is not met.

The same rule drives the Submissions tile, the month report, the team
report's submission column, the monthly submission reminder, and the
scheduled timesheet PDF upload. They can never disagree with each other.

### Important: non-crediting entries affect completeness

Non-crediting entries count toward the submission check just like crediting
entries. If you have a non-crediting entry in draft, your week remains
**incomplete** until you submit it.

**Example:**

- Monday–Friday: all crediting work entries submitted/approved
- Wednesday: one team meeting (non-crediting) still in draft
- Week status: **Incomplete** — Wednesday's draft blocks the whole week
- Once you submit Wednesday's meeting, the entire week becomes **Complete**
- Flextime calculation then includes Mon–Tue, Thu–Fri crediting entries only
  (the non-crediting meeting is not counted in flextime regardless)

States:

- `All submitted and approved` (green): every elapsed week has been submitted and all entries are approved (no pending approvals remaining).
- `All submitted (approvals pending)` (orange): every elapsed week has been submitted, but at least one entry is still waiting for approval.
- `Weeks missing` (orange): at least one elapsed week has missing or unfinished submissions.

## Leave accounts and carryover logic

Each absence category that uses a leave account has its own independent yearly
budget. Vacation remains one leave account; an organisation can add accounts
such as educational leave without mixing their balances.

### Balance cards

The Absences page and personal report show one card per leave account. Each
card contains the category, annual entitlement, carryover, already taken,
approved upcoming, requested, available amount, and any remaining carryover.
The team report has one compact taken/planned column per account.

| Field | Meaning |
| --- | --- |
| Annual entitlement | The user's entitlement for this account and selected year, after any start-date proration. |
| Carryover days | Unused days from the same account in previous years. |
| Carryover expiry | The account's own month and day on which carryover expires. |
| Already taken | Approved account days in the past or today. |
| Approved upcoming | Approved account days after today. |
| Requested | Requested and cancellation-pending account days. |
| Available | Usable budget after all reserved account days. |

### Entitlement, start year, and carryover

An account starts for a user in the later of the user's Zerf start year and the
account's internal start year. Before that year it has neither entitlement nor
carryover. This prevents a newly added category from creating historic
entitlements.

Within a valid year, entitlement uses the account's user-specific base value
or a yearly override. If the hire date, or otherwise the Zerf start date, falls
in that year, entitlement is pro-rated. Each account has its own carryover
chain and expiry date; changing an account's expiry affects newly calculated
balances immediately, including historic views.

Approved, requested, and cancellation-pending absences reduce the source
year's carryover. Rejected and cancelled absences have no balance effect. A
cancellation remains reserved until it is decided.

If an absence crosses New Year's Day, Zerf validates each affected year for
the same leave account. Days after an account's carryover expiry must fit into
the remaining annual entitlement.

## Notifications

Notification titles, bodies, and email wording use the interface language that
is configured when Zerf creates the notification.

### Employee receives notifications when

- a week is approved or rejected (one notification per action, identifying the affected weeks),
- absence is approved or rejected,
- absence cancellation is approved or rejected,
- reopen request is approved or rejected,
- a monthly submission reminder is triggered on the configured deadline day (lists past weeks that are still not submitted).

If an admin approves or rejects their own item, Zerf records the audit event
and sends an in-app-only notification (no email) back to the same user.

### Approver receives notifications when

- a week is submitted (one notification identifying the submitted weeks),
- an absence request is submitted (the notification and email include the
  employee's comment, if one was added),
- a reopen request is submitted,
- a weekly approval reminder is triggered (pending items awaiting review).

### Exception: auto-approved submissions and reopen requests are silent

When a user has submission or reopen auto-approval enabled (see Team settings
below), the corresponding action is recorded in the audit log as usual, but
**no in-app notification and no email are sent to anyone** — neither the
requester nor their approvers. This is different from every other
auto-approval in Zerf (e.g. past-dated sick leave), which still notifies the
approver informationally; submission and reopen auto-approval are
intentionally silent end-to-end.

### Who gets notified

- Only explicitly assigned approvers receive approval notifications and reminders.
- If a user has multiple active approvers, each of them receives the same
	request notification.
- Admin notifications and reminders are sent based on explicit assignment.
- Inactive approvers are skipped.
- For admin-subject workflows, only admins can act on the request.

This also applies to reminder emails and in-app reminders: Zerf reminds the
users who are actually assigned, not all users with a privileged role.

### Pending approval notifications clear automatically

As soon as a request has been decided by any one approver (approved, rejected,
revoked, or the cancellation thereof) — or is otherwise removed from the
queue by the requester (withdrawn, or edited into auto-approval) — the
related notification is marked as read for every other approver in the same
instant. This applies to:

- week submissions for time entries,
- absence requests and absence cancellation requests (including an employee
  withdrawing a `requested` absence, and an edit that flips a future-dated
  sick request into auto-approval),
- reopen requests.

The notification row stays in each approver's notification history (and the
audit log keeps the full trail), but the item no longer shows up in the
unread badge or in the dashboard's "open requests" lists for anyone else. So
once a colleague has acted, you do not need to refresh or re-check whether
your action is still required.

Week submission notifications are tracked per week. If one submitted week is
approved or rejected while another week from the same person is still pending,
only the decided week's notification is marked read.

### Important: non-crediting entries trigger reminders too

Because non-crediting entries participate in the full approval workflow:

- **Submission reminders** go to employees who have ANY incomplete entries (crediting or non-crediting).
  - If you have unsubmitted crediting work and unsubmitted non-crediting meetings, you receive the reminder.
  - If you have only unsubmitted non-crediting entries, you still receive the reminder (to complete the workflow).

- **Approval reminders** go to approvers when there are submitted entries awaiting their decision.
  - Approvers see and must review both crediting and non-crediting entries.
  - Approval reminders are triggered by any submitted entry type.

Duplicate reminders are automatically suppressed — you will not receive the same reminder twice for the same day.

### Monthly submission reminder

On the configured submission deadline day each month, every active user with weekly hours > 0 (employees and team leads alike) receives one reminder (in-app, plus email if SMTP is enabled) listing the past weeks that are not fully submitted up to that day. The current week is excluded.

The reminder is sent directly to the affected user, not to their approvers. Duplicate reminders for the same user and deadline day are suppressed.

**What triggers the reminder:**
- Any required workday in a past week not covered by a submitted/approved entry or a requested, approved, or cancellation-pending absence.
- Days with only draft or unresolved rejected entries count as incomplete.
- Non-crediting entries fully participate: a day covered only by an unsubmitted non-crediting entry keeps the week incomplete.

### Weekly approval reminder

Zerf can send a weekly reminder to approvers when submitted items are waiting
for review.

- Reminder day/time follows the configured app timezone.
- Recipients are explicit active assignees only.
- Duplicate reminders for the same day are suppressed.

**What triggers the reminder:**
- Any submitted (not approved/rejected) entries from any category type.
- Non-crediting entries are included: if an approver has pending non-crediting entries, the reminder is sent.

### Reminder toggles (admin)

Admins can manage reminder behavior in Settings → General:

- submission reminders enabled/disabled,
- approval reminders enabled/disabled.

These toggles control whether the corresponding reminder background task sends
notifications/emails.

### System error notifications (admin)

When a technical failure occurs — such as a database backup failure, a Nextcloud upload error, or any error logged by the application — admins can be alerted. This is **opt-in per admin**: only admins whose profile has **"Receives notifications about technical system errors"** enabled are notified. The option is off by default and is set when creating or editing an admin user (it appears only for the Admin role).

- Opted-in admins receive both a **pinned** in-app notification (highlighted at the top of the notification panel) **and an email**.
- If no admin has opted in, technical errors are still recorded in the System Log but no one is notified — enable the option for at least one admin to receive alerts.
- Each failure class produces **at most one active notification** per admin. If the notification is dismissed and the failure recurs, it is raised again.
- If no email server (SMTP) is configured, the in-app notification is still created and the missing email is noted in the System Log; no delivery is retried endlessly.
- **Backup and upload failure notifications are automatically resolved** when the next cycle succeeds. You do not need to dismiss them manually after fixing the underlying problem; the notification disappears on the next successful backup or upload.

### Notification timestamp display

Notification and email timestamps shown to users are rendered in the configured
app timezone so users see consistent local business time.

## Important edge case: sick leave with existing time entries

If approved absence overlaps a day with recorded work:

- daily target becomes `0`,
- existing time entries still count as actual worked hours.

Result: the day can produce a positive flextime delta.

This is intentional. It supports cases like partial sick days where someone worked part of the day.

The same mechanics apply to public holidays: logging time on a holiday is
allowed (someone may work or be on call that day). The daily target stays
`0`, so any logged hours become a pure flextime gain, exactly like the
sick-day case above.

## Approval structure examples

### Role organigram

```mermaid
flowchart TD
	Admin[Admin]
	LeadB[Approver team lead]
	LeadA[Team lead]

	subgraph TeamGroup[Operational team]
		E1[Employee 1]
		E2[Employee 2]
		EN[Employee n]
	end

	LeadA -->|approver for| E1
	LeadA -->|approver for| E2
	LeadA -->|approver for| EN
	LeadB -->|approver for| LeadA

	Admin -->|manages platform and users| LeadB
	Admin -->|can be explicitly assigned as approver| LeadA
```

### Example approval flow

```mermaid
flowchart LR
	Employee[Employee submits request]
	Lead1[Assigned team lead 1]
	Lead2[Assigned team lead 2]
	LeadApprover[Approver team lead]
	Approved[Approved]
	Rejected[Rejected]
	LeadOwn[Team lead submits own request]

	Employee -->|any assigned active approver can review| Lead1
	Employee --> Lead2
	Lead1 -->|approve| Approved
	Lead1 -->|reject| Rejected
	Lead2 -->|approve| Approved
	Lead2 -->|reject| Rejected

	LeadOwn -->|admin-subject requests require admin reviewers| LeadApprover
	LeadApprover -->|approve| Approved
	LeadApprover -->|reject| Rejected
```

### What explicit assignment means

When an approver is assigned to a user:

- that approver receives the user's approval-related notifications,
- that approver can review the user's submitted requests,
- the user appears in that approver's visible team scope,
- the assignment must point to an active user.

When no approver is assigned:

- no approver notification route exists,
- no review queue entry is created for that relationship,
- non-admin users should be configured with at least one approver.

For admins, the assignment list matters for notifications. If an admin is
not explicitly assigned, they will not receive approval reminders or request
notifications just because they are an admin.

## Reporting behavior (important)

Zerf distinguishes between workflow coverage and work-credit math.

### Reports page layout

- Employees without team-report access see a single report: their own balance,
  vacation, absences, category breakdown, entries, and flextime chart.
- The entries table includes a **Comment** column showing any note attached to
  an entry. Long comments are shortened with an ellipsis; hover over one to read
  the full text. Entries without a comment show a dash.
- Team leads and admins additionally see an **Employee** / **Team** switch at
  the top of the page. The Employee tab shows one person's report at a time
  (with a dropdown to pick who); the Team tab shows the whole team side by
  side: a per-person balance table, a category breakdown across everyone, and
  team absences.
- A single toolbar above the report controls both tabs: pick a month with the
  ◀ / ▶ arrows, or switch to **Custom range** to pick any from/to date span
  (useful for a quarter, or for looking ahead at planned absences beyond the
  current month). Switching tabs keeps the selected person and period.
- Everything loads automatically as soon as you change the employee or the
  period — there is no separate "Show" button.
- **CSV** and **PDF** export buttons in the toolbar export exactly what is
  currently on screen: the selected employee and period on the Employee tab,
  or a combined PDF for the whole team on the Team tab.
- Absences look forward: picking a custom range that extends into the future
  still shows planned/approved time off in that range. Worked hours, flextime,
  and exports never include future days — a period entirely in the future
  shows a note instead of empty balance figures.

### Month and overtime/flextime math

- Work-credit calculations use only entries that count as work and match the
	relevant status rules (for example approved for actuals).
- Non-crediting entries remain visible in workflow but do not inflate worked
	hour balances.
- The flextime balance is always calculated up to and including yesterday;
	today is excluded everywhere the balance is displayed (dashboard, reports,
	team overview, balance chart, CSV/PDF exports).
- Flextime balance charts mark absences, public holidays, and weekends with
	colored background bars so non-working days are visible in the timeline.
	Today's data point is included on the chart axis but contributes zero to
	the running balance until tomorrow.

### Category breakdown reports

- Category breakdowns show all booked non-rejected time entries in scope (not only
	crediting categories).
- This gives a complete operational view of what was booked by category.
- Employees see their own breakdown. Leads and admins can view a team aggregate
  for active time-tracking users in their reporting scope.

### Team report scope

- Admins can see all active users who track time.
- Non-admin leads see themselves plus explicitly assigned direct reports.
- Non-admin leads do not see admin subjects in lead-scoped team reporting.
- Personal report endpoints (month, range, CSV export, categories, overtime,
	flextime) are available only for active users who track time. Pure-admin
	accounts and inactive users do not have reportable personal datasets.

## Admin checklist for a correct setup

Use this checklist after initial deployment or major configuration changes.

1. Set app timezone in settings (Settings → General).
2. Assign explicit active approvers for all non-admin users.
3. Review reminder toggles (submission and approval reminders).
4. Confirm holiday data is loaded for current/next year.
5. Validate one end-to-end flow:
	 employee submits week -> approver receives notification -> approver reviews.

If one step is missing (especially explicit approver assignment), approval
notifications and pending queues will not behave as expected.

## FAQ

### Why can my approver not see my entries?

Your week is likely still in `Draft`. Approvers only review after `Submit Week`.

### Why was my absence rejected even though dates were valid?

Common reasons:

- range contains no effective workday,
- non-sick absence overlaps existing time entries. The request itself is
  accepted, but the approver is blocked from approving it until the
  conflicting entries are removed or rejected.

### Why does my flextime increase on a sick day?

Because approved absence sets target to `0`, and recorded work still counts as actual time.

### Why does submission status show missing weeks even though current week is in progress?

Current week is excluded. Missing status is based on incomplete past full weeks.

### Why don't the hours I booked today change my flextime balance?

The flextime balance is intentionally calculated up to and including yesterday
only. Today's hours move the balance starting tomorrow. This avoids a balance
that shifts up and down during the day as hours are logged. Your today entries
still appear in the time entry list and in the monthly logged-hours tile;
they simply do not contribute to the balance yet.

---

## Employee workflow reference

This section documents every action an employee can perform, with the exact
rules enforced by the system.

### Recording time entries

**Create a time entry**

A time entry requires a date, a start time, an end time, and a category. The
following rules apply:

- The date must be today or in the past (future dates are not allowed).
- The date must be on or after your employment start date.
- End time must be later than start time.
- When the date is today, end time must not be in the future.
- The time range must not overlap with any existing non-rejected entry on the
  same day.
- The day must not be covered by a non-sick absence that is approved,
  pending cancellation, or still awaiting approval. Sick-like (auto-approve)
  absences do not block time entry creation; all other absence types do.

There is intentionally **no maximum number of hours per day** and no limit on
the length of a single entry. Zerf records whatever hours were actually worked —
long or on-call days are legitimate, and assistants (see the Roles section
above) have no target to measure against anyway.

A new entry is always created in draft status.

**Edit a time entry**

Only `draft` entries can be edited directly. Submitted, approved, or rejected
entries are part of a locked week — to change them you must first reopen the
whole week via a reopen request (see below). The same validation rules as
creation apply.

**Delete a time entry**

Only `draft` entries can be deleted. Submitted, approved, or rejected entries
cannot be deleted directly; use a reopen request to make them editable first.

### Submitting a week

`Submit Week` transitions a set of draft entries to `submitted` so that
approvers can review them.

Rules:

- Only your own draft entries are submitted; entries in another status are skipped.
- Once submitted, entries are locked for direct editing.

After submission, all your explicitly assigned approvers receive a notification
identifying the submitted weeks by their week labels.

**Auto-approval:** If your team lead or admin has enabled auto-approval of
submissions for you (Settings → Team Settings → "Auto-approve submissions"), submitted
weeks skip the approval queue entirely and go straight to `approved`. This is
silent by design: neither you nor your approvers receive any notification or
email about it.

### Requesting a week reopen

`Request edit` (a "reopen request") is the only way to amend a week after
submission. There is no per-entry change-request workflow — the week is the
unit of approval.

Rules:

- The week must contain at least one submitted, approved, or rejected entry.
  A week that is entirely draft is already editable and cannot be reopened.
- You cannot submit a second reopen request for the same week while one is
  still pending.

**Submitted but not yet approved:** If all reopenable entries in the week are
still waiting for approval as `submitted`, `Request edit`
reopens the week immediately. The submitted entries are reset to `draft`, the
obsolete submission approval is removed from the approver queue, and no separate
edit request is shown to approvers. If the same week already contains approved
or still-unresolved rejected entries and also still has submitted entries
awaiting approval, finish or reopen the pending submission first; Zerf will not
create a parallel edit request for that mixed state.

**Auto-approval:** If your team lead or admin has enabled auto-approval for your
reopen requests, the reopen takes effect immediately without requiring approval.
This is silent by design: neither you nor your approvers receive any
notification or email about it.

**Manual approval path:** If the week has no submitted entries left and reopen
auto-approval is not enabled for you, the request enters `pending` status and
all your assigned approvers are notified.

When a reopen is executed (either path), submitted and approved entries are
reset to `draft`. Rejected entries are also reset when they have not already
been replaced by a submitted or approved entry on the same day. You can then
edit and resubmit the week.

### Absences: creating

**Allowed absence kinds:**
Vacation, sick leave, training, special leave, unpaid leave, general absence, and flextime reduction.

**Rules that apply to all kinds:**

- End date must be on or after start date.
- The range must not exceed one year.
- The range must include at least one effective workday. An effective workday
  is a potential workday (based on your configured days per week) that is not
  a public holiday. A request covering only non-workdays or public holidays is
  not valid.
- Start date must be on or after your employment start date.
- Comment, if provided, must not exceed 2000 characters.

**Additional rules for leave-account categories:**

- The balance of the selected leave account is validated for every year
  covered by the request. Insufficient balance blocks the request.

**Additional rules for sick leave:**

- Start date cannot be more than 30 days before today.
- If the start date is today or earlier: sick leave is **auto-approved** immediately.
  Your approvers receive an informational notice, in-app and by email (not an action request).
- If the start date is in the future: sick leave requires approval like any other absence.

**Overlap and time-entry conflict:**

- Any absence overlapping another existing absence is rejected.
- A non-sick absence (vacation, training, etc.) that overlaps days with
  existing time entries can still be *requested*. The conflict is checked at
  **approval** time, not at creation. The approver cannot approve it until the
  conflicting entries are removed or rejected (see [Overlap
  rules](#overlap-rules)). Once the request is pending, creating *new* time
  entries on the covered days is blocked so the conflict cannot get worse.
- Sick leave overlapping existing time entries is allowed. The daily target
  becomes 0 for covered workdays, but the existing entries still count as
  worked hours.

After creation, assigned approvers receive a notification if the absence
entered `requested` status.

### Absences: editing a pending absence

Only absences in `requested` status can be edited. Approved absences cannot be
edited; cancel and re-request instead.

- The absence kind cannot be changed to or from `sick`.
- All creation validation rules apply to the updated values.

If the updated absence remains in `requested` status, approvers are notified
of the change. If it transitions to `approved` (sick leave with start_date
today or earlier), approvers receive an auto-approval notice.

### Absences: cancelling

The cancellation path depends on the current absence status:

| Current status | Action | Effect |
| --- | --- | --- |
| `requested` | Cancel | Immediate: status becomes `cancelled`. Approvers notified that request was withdrawn. No approval needed. |
| `approved` | Cancel | Deferred: status becomes `cancellation_pending`. Approvers notified to review the cancellation. Budget still reserved. |

Only `requested` and `approved` absences can be cancelled. Already cancelled,
rejected, or cancellation-pending absences cannot be cancelled again.

### Leave accounts

Your leave-account balances are visible in the leave overview. There is one
card for every account available to you. The balance fields are:

- **Annual entitlement**: configured leave days for the year, pro-rated if you
  started during the year.
- **Carryover days**: unused days from the same account carried over from the
  previous year (never below zero).
- **Carryover expiry**: date after which that account's carryover is no longer
  usable.
- **Already taken**: approved account days that are today or in the past.
- **Approved upcoming**: approved account days that are in the future.
- **Requested**: days in `requested` or `cancellation_pending` status.
  Budget is still reserved.
- **Available**: total usable budget − already taken − approved upcoming −
  requested.

Cross-year requests are validated per year: days in year Y consume that
account's budget for Y, while days in year Y+1 consume its budget for Y+1.

---

## Team lead workflow reference

Team leads (role `team_lead`) and admins both have lead privileges. Unless
otherwise noted, all lead actions below apply to both roles.

### Scope of lead authority

Non-admin team leads can only act on users who are explicitly assigned to them.
This applies to:

- Viewing the team list
- Reviewing time entries, absences, and reopen requests
- Team reporting

Admin users can see and act on all users.

**Self-review restriction:** Non-admin leads cannot approve or reject their
own time entries, absences, or reopen requests. Their own submitted entries
are not shown in the Dashboard approval queue. Admins may approve or reject
their own items, and their own submitted entries appear in the queue like
any other user's.

**Admin-subject rule:** Non-admin leads cannot act on items submitted by admin
users. Admin-subject requests require an admin reviewer.

### Reviewing time entries (week level)

Approve or reject submitted time entries. All approval and rejection operates
at the week level. The week is the primary reviewable unit; individual entries
within a week are handled in the background.

- For approve: only submitted entries for users within your scope are changed.
  Entries outside your scope or in a different status are skipped.
- For reject: a rejection reason is required. The reason applies to all rejected
  entries in the batch.
- Non-admin leads cannot approve or reject their own entries. Admins may approve
  their own entries.
- Non-admin leads can only act on their direct reports' entries.
- Employees receive one notification per approval or rejection, identifying
  the affected weeks. Admins who review their own entries receive an in-app
  notification only (no email).
- Entries from users with submission auto-approval enabled never reach this
  queue — they are approved at submission time, silently (see [Team settings:
  submission policy](#team-settings-submission-policy)).
- When you open a pending week, the review dialog offers a `View in report`
  button. It takes you straight to that employee's detailed report for exactly
  that week, so you can inspect every entry — including any comments the
  employee added — before approving or rejecting.

### Reviewing an absence

Approve or reject an absence in `requested` status.

- You must be a team lead or admin.
- Non-admin leads can only act on direct reports' absences.
- Non-admin leads cannot approve/reject their own absence.
- Only `requested` absences can be approved or rejected.
- Rejection requires a reason (non-empty, max 2000 characters).

**Leave-account re-validation at approval time:** When approving an absence
booked against a leave account, the system re-validates that account's balance
against the employee's current entitlement. If another absence in the same
account was approved in the meantime and exhausted the budget, approval is
blocked.

**Time-entry conflict check at approval:** For non-sick absences, the system
re-checks that no time entries exist on the covered days at approval time. If
entries were created after the request was submitted, the approval is blocked.

### Reviewing an absence cancellation

Approve or reject a `cancellation_pending` absence.

- You must be a team lead or admin.
- Non-admin leads can only act on direct reports' absences.
- Non-admin leads cannot act on their own absence.
- Only `cancellation_pending` absences can have their cancellation reviewed.

| Decision | Result | Employee notification |
| --- | --- | --- |
| Approve cancellation | Absence status → `cancelled`. Budget released. | Yes (unless self-action by admin) |
| Reject cancellation | Absence status → `approved` (restored). Budget still consumed. | Yes (unless self-action by admin) |

### Reviewing a reopen request

Approve or reject a `pending` reopen request.

- You must be a team lead or admin.
- Non-admin leads can only act if explicitly assigned as approver for the
  requesting user.
- Non-admin leads cannot approve or reject their own reopen request.
- Only pending reopen requests can be reviewed.
- A rejection reason is required for rejection.

On approval: the reopen is executed atomically. Submitted and approved entries,
plus rejected entries that still need correction, are reset to `draft`. Rejected
entries already closed by a correction stay as history. The employee receives a
notification.

On rejection: the week remains unchanged. The employee receives a rejection
notification with the reason.

Auto-approved reopen requests (see below) never reach this review queue — the
reopen already happened at request time, silently.

### Team settings: reopen policy

Team leads and admins access these settings via Settings → Team Settings.

Team leads can enable or disable auto-approval of reopen requests for their
direct reports. They only see users for whom they are assigned as approver.
Admins can see and set it for any user (including themselves).

Non-admin team leads cannot modify their own reopen policy — only their own
approver (a higher lead or admin) may grant them auto-approval. This prevents
a lead from bypassing their own approval chain.

- When enabled: that user's future reopen requests are auto-approved immediately.
- When disabled: reopen requests require manual approval.
- Changes are recorded in the audit log.
- Auto-approval is silent: no notification or email is sent to the requester
  or to their approvers (see [Notifications](#notifications)).

### Team settings: submission policy

Team leads can enable or disable auto-approval of timesheet submissions for
their direct reports. They only see users for whom they are assigned as
approver. Admins can see and set it for any user (including themselves).
This is an independent setting from the reopen policy above — a user can have
either, both, or neither enabled.

The same self-service restriction applies: non-admin team leads cannot modify
their own submission policy.

- When enabled: that user's submitted weeks skip the `submitted` status and go
  straight to `approved`. The system records the user themselves as reviewer.
- When disabled (default): submissions require manual approval as usual.
- Changes are recorded in the audit log.
- Auto-approval is silent: no notification or email is sent to the requester
  or to their approvers (see [Notifications](#notifications)).

### Viewing team reports

Team leads can access team-scoped reports covering their direct reports plus
themselves.

- Report date ranges are validated: from must be ≤ to, and the date span must
  not exceed 366 days.
- Non-admin leads see only users explicitly assigned to them (plus themselves).
  Admin users are not visible in non-admin lead team reports.
- Admins see all active users who track time.
- The timesheet export can produce a single combined PDF for the whole team
  (rather than one user at a time). Its per-user sections are ordered by role
  — team leads, then employees, then assistants, then admins — and
  alphabetically within each role, matching the on-screen user lists.
- The PDF table shows every non-rejected entry with a **Status** column (Draft,
  Submitted, or Approved) so you can see which entries are included in the
  **Total (approved)** row at the bottom. The Total counts only approved,
  work-crediting entries after automatic break deduction — entries that are
  still draft or submitted, or belong to a non-crediting category, appear in
  the Duration column but are not counted in the Total.

### Scoped assistant user management (optional)

An admin can enable **Allow team leads to create assistant users** in Settings
→ General (see [System settings](#system-settings)). This is off by default.

When enabled, every non-admin team lead gets an additional **Users** tab under
Settings, scoped strictly to their own assigned users:

- The list shows everyone assigned to the lead as approver, plus the lead
  themselves — including archived direct reports (unlike every other
  lead-facing list, which only shows active team members; this is needed so a
  lead can find and restore an assistant they previously archived). For
  anyone who is **not** an "Assistant" (including the lead's own row), only
  the name is shown; no other field is sent by the server, and no action is
  available.
- Only users with role "Assistant" who are assigned to the lead can be viewed
  in detail, edited, archived, or restored.
- There is no delete action here. A team lead can never delete a user — only
  an admin can, via the regular Users tab.
- The **Add User** button only lets the lead create a new "Assistant" user.
  The role field is fixed and cannot be changed, and the new user's approver is
  always the creating lead — no other role or approver can be chosen.
- Admins are unaffected: they continue to use the full Users tab and can
  create, manage, or delete users with any role, as described under [Admin
  workflow reference](#admin-workflow-reference).

This is enforced by the backend, not just hidden in the UI: every action a
non-admin lead can perform here is re-validated against the assigned-assistant
scope on the server, regardless of what the client sends (see [Security and
access control](#security-and-access-control)).

---

## Admin workflow reference

Admins have all team lead privileges plus exclusive access to user management,
system settings, and sensitive operations.

### Reading the audit log

- Audit rows are shown in a human-readable format (for example names, date
  ranges, category names, or setting keys) instead of raw field dumps.
- Submitting, approving, or rejecting a week each appear as a single row,
  because that is how a week is actually decided — one row names the week,
  how many day entries it covers, and (for a rejection) the reason. Clicking
  it opens every day entry the decision covered, with its time range,
  category, and comment. Adding, editing, or deleting a single day entry
  shows up as its own separate row instead, since that is an individual
  action, not a week-level decision.
- If the acting user has since been deleted, the actor column shows a
  placeholder instead of a name. The audit record itself is preserved.
- Entries are listed newest first, 100 per page. Use the pager below the list
  to move between pages.
- Click an entry to see its details in a popup.

### Reading the system log

Settings > System Log shows warnings and errors that occurred while the
application was running — for example a failed email delivery or an
unreachable holiday service. It helps admins understand why something did not
work without needing access to the server.

- Only warnings and errors are collected; routine activity is not logged here.
- Entries are listed newest first, 100 per page. Long messages are shortened
  in the list — click an entry to see the full message with its context in a
  popup.
- The log keeps at most 1000 entries, and entries are removed after one year.
  Older entries are deleted automatically.

### Creating a user

Creating a user requires admin role.

Required information:

- Role (employee, assistant, team lead, or admin)
- Email address (must be unique)
- First and last name (the combination must be unique)
- Weekly hours and workdays per week
- Leave-account entitlements and the current and next year's account overrides
  (see [Managing leave accounts](#managing-leave-accounts))
- Employment start date

Role-specific rules:

- Assistants must have zero weekly hours and no overtime start balance. The
  corresponding fields are hidden in the form.
- When the assistant role is selected, leave-account entitlements are reset to
  0. This is intentional: under German law, Minijob (assistant) leave
  entitlement is derived from the number of days actually worked, which varies
  per person and per year. Enter the correct entitlement for each account and
  year manually.

Optional: an initial flextime balance to carry in from before the user was
created in the system.

Optional: a hire date — the date the person actually joined the company, if it
differs from their start date in Zerf. This matters when introducing Zerf to an
existing team: someone who already worked the full year before adopting Zerf
should see their full account entitlement, not one pro-rated from the day they
started using Zerf. Set the hire date to their real employment start, and each
account entitlement is pro-rated from that date instead. Leave it empty to
pro-rate from the start date, which is correct when employment and Zerf usage
begin on the same day.

Optional: one or more approvers to assign to the new user.

Optional (admin role only): **Receives notifications about technical system
errors**. When enabled, this admin is alerted — in the app and by email —
whenever a technical error occurs (see
[System error notifications](#system-error-notifications-admin)). It is off by
default and only appears when the Admin role is selected; the same option is
available later when editing the admin.

Optional: which time categories and absence categories the new user can use.
The creation form shows both lists pre-checked (every existing category
enabled, the default), so deselecting one is the only action needed to
restrict it. See [Managing categories](#managing-categories) for how access
is adjusted later, after the user already exists.

A temporary password is generated automatically. The user must change it on
first login. A registration email with the temporary password is sent if
email delivery is configured.

After creation, assign at least one active approver for non-admin users so that
approval routing works.

### Updating a user

Only provided fields are changed when updating a user. The same rules as
creation apply to each field.

**Guards to prevent accidental lockout:**

- An admin cannot set their own role to a non-admin value.
- Removing admin rights from a user requires at least one other active admin
  to remain after the change (the last active admin can never be demoted).
- A user who still has *active* direct reports assigned cannot have their role
  changed to a non-approver role. Reassign those users first. Archived
  dependents (which keep their approver link for restore purposes) do not
  count toward this guard.
- A user who is the sole approver for *active* admin users cannot be
  downgraded to a role that cannot approve admin-subject requests.

When a user's role is changed, they are signed out immediately.

### Archiving a user

When a user has historical data (time entries, absences, or requests), the
system blocks permanent deletion and requires archiving instead. Archiving is
a soft removal that preserves all history while preventing the user from
logging in and hiding them from active lists.

**Location:** Settings > Users > select a user > Archive.

What archiving does:

- The user can no longer log in; any active sessions end immediately.
- The user disappears from the normal user list, approver pickers, team
  reports, and dashboards.
- All historical time entries, approved absences, and audit records are kept.
- Time entries still waiting for approval (`submitted`) are reverted to
  `draft` so they leave every approval queue and stop triggering approval
  reminders. Approved and rejected entries are untouched.
- All pending absence requests and pending reopen requests owned by the user
  are auto-rejected with the reason "User account archived."
- Already-approved absences (including future ones) are preserved.

Guards:

- Cannot archive yourself.
- Cannot archive the last active admin.
- If the user is currently listed as an approver for any active users, you
  must provide a replacement approver for each of those users in the same
  request. The archive is rejected unless every dependent user has a valid
  replacement assigned.

**Viewing archived users:** Settings > Users. The archived accounts are shown
in a separate "Archived Users" list below the active users on the same page.
Each row shows the user's name, role, and the date they were archived.

### Restoring an archived user

Restore brings an archived user back as an active account.

**Location:** Settings > Users > Archived Users list > select a user > Restore.

Restore behavior:

- The user becomes active again and can log in.
- `must_change_password` is set to true, so the user is forced to set a new
  password on first login.
- Approver assignments must be provided as part of the restore request; the
  user will have no approvers until they are set here.
- Optionally, a new start date can be supplied. This resets the start date
  used for flextime and balance calculations, which avoids accumulating a
  flextime gap for the period the user was archived. If no new start date is
  given, the original start date is kept.
- Historical data created before archiving is unchanged.

### Deleting a user

Permanent deletion removes all user data (entries, absences, requests, leave
records).

If the user has any historical time data, deletion is blocked and the error
message instructs you to use archiving instead.

Guards:

- Cannot delete yourself.
- Cannot delete the last active admin.
- Cannot delete a user who is still listed as an approver for active users.

There is no undo. Use archiving if you want to preserve history.

### Resetting a password

Resets the password for any active user.

- Only active users can have their password reset.
- A new temporary password is generated automatically.
- The user is required to change it on next login.
- All existing sessions for that user end immediately.
- When SMTP is configured, the user receives an email with the new temporary
  password. When SMTP is not configured, the admin must deliver the password
  to the user manually.

### Managing approver assignments

The approver list for a user controls who receives notifications and can
review that user's requests.

Rules for valid approvers:

- The approver must be an active user.
- The approver must have the team lead or admin role.
- Non-admin approvers cannot review admin users (even if assigned). Only
  admins can act on admin-subject requests.

When an approver is removed, the change takes effect immediately for future
requests. Pending requests that were already routed are not re-routed;
previously notified approvers can still act on them.

Non-admin users without an assigned approver cannot submit time entries,
absence requests, or reopen requests.

### Direct correction of submitted or approved entries

Admins can directly edit a submitted or approved time entry that belongs to
another user, without going through the reopen workflow. This is the admin
correction path.

Rules:

- The admin must be a different user from the entry owner (not editing their own entry).
- Entry status must be submitted or approved.
- The same validation rules as time entry creation apply, with one exception:
  the per-user category enablement check is skipped. An admin correction may
  assign a category that is disabled for that specific employee (only an
  inactive category is still rejected); it does not, by itself, re-enable the
  category for the employee's own future entries.

Admins editing their *own* submitted or approved entries must instead go through
the regular reopen workflow — the admin correction path only applies to other
users' entries.

### Managing leave accounts

Every category configured as a leave account has an independent base
entitlement and optional per-year overrides for each user:

- **Base days**: the user's standing entitlement for that account, used for any
  year that has no explicit override.
- **Per-year overrides**: explicit account days for a specific year. When set,
  an override takes precedence over that account's base value for that year.
- Valid range: 0 to 366 days for both base values and overrides.
- **Assistants (Minijob)**: leave entitlement must be set manually each year
  because it is calculated from the actual number of days worked, which
  changes from year to year. Account values default to 0 when the assistant
  role is selected. Update the current-year and next-year overrides each
  January once the number of worked days for the previous year is known.
  Assistants have no fixed contract workdays (they are configured with all 7
  days as potential working days), so every calendar day in a leave-account
  request — weekends included, public holidays excluded — counts as one leave
  day against their entitlement.
- Changes take effect immediately for balance calculations. If you reduce a
  user's account entitlement after they have already used account days, their
  available balance may go negative.
- When onboarding someone who already worked for the company before adopting
  Zerf, set their hire date (in the user's edit dialog) to their real
  employment start. This anchors leave proration on that date instead of their
  (later) Zerf start date, so they see their full entitlement rather than one
  wrongly pro-rated from when they started using Zerf.

### Revoking an approved absence

Admins can forcibly cancel an approved absence (for example, to fix a mistaken
approval).

- Only approved absences can be revoked.
- The absence is cancelled and the absence owner receives a notification
  (unless the admin is revoking their own absence).
- Budget freed by the revocation is reflected immediately in the balance
  calculation.

Revoke is distinct from cancellation: employees request cancellation;
admins revoke.

### System settings

Admins configure system-wide behavior in the Settings panel (Settings → General):

| Setting | Description |
| --- | --- |
| App timezone | Timezone name (e.g. `Europe/Berlin`). All date logic uses this timezone. |
| Submission deadline day | Day of the month (1–28) when the monthly submission reminder is sent. |
| Submission reminders enabled | Enable or disable the monthly submission reminder. |
| Approval reminders enabled | Enable or disable the weekly approval reminder. |
| Automatic break deduction | When enabled, deducts a configured break from each day where consecutive crediting work exceeds a threshold. See [Automatic break deduction](#automatic-break-deduction). |
| SMTP configuration | Server, port, and credentials for outgoing email. Required for registration emails and email reminders. |
| Public URL | Used to construct login links in registration emails. |
| Nextcloud Upload | Configure automatic upload of encrypted DB backups and monthly timesheet PDFs to a Nextcloud public share. See [Nextcloud Upload](#nextcloud-upload). |
| Payroll Report | Email a monthly PDF with absence days and working hours to your payroll accountant or tax office. See [Payroll Report](#payroll-report). |
| Allow team leads to create assistant users | Off by default. Only an admin can change it. When on, non-admin team leads get a scoped Users tab limited to creating/managing "Assistant" users assigned to them. See [Scoped assistant user management (optional)](#scoped-assistant-user-management-optional). |

### Nextcloud Upload

Zerf can automatically upload two types of files to Nextcloud shared folders using public share links.

#### DB Backup Upload

When enabled, the backup container uploads the backup archive (a `.zip` file) to a Nextcloud public share immediately after it is created. Each archive bundles the encrypted database dump, a plaintext metadata record, and (where available) the encrypted pg_tde keyring into a single file for easy off-site storage.

| Setting | Description |
| --- | --- |
| Enable DB backup upload | Activates the upload step in the backup container. |
| Share link | A Nextcloud public share URL in the form `https://cloud.example.com/s/<token>`. Only `https` links are accepted. |
| Share password | Optional password protecting the share. Stored securely; never returned by the API. |
| Backup interval (days) | How often the backup container runs a backup cycle. Default: 1 (daily). Changes take effect within one hour. |

The backup container tracks the last successful backup time in the database. This timestamp survives container restarts, so the interval is always measured from the last actual backup rather than from container start time. On a fresh install, migration 024 seeds the timestamp with the current time, so the first backup runs one full interval after setup (not immediately).

The **10 most recent** local backup archives are kept in the backup volume; older ones are deleted automatically after each successful backup. Uploaded files in Nextcloud are **not** deleted automatically — manage the shared folder manually to avoid unlimited growth.

The database dump inside the archive is AES-256-CBC encrypted, so a compromised share link does not expose plaintext data.

If a backup fails, admins who opted in to technical error notifications are alerted in the app and by email (see "System error notifications"). The notification is automatically re-raised if it was previously dismissed and the failure recurs.

#### Report PDF Upload

When enabled, Zerf queues an individual timesheet PDF for each employee on a configurable day each month. Each PDF covers the **previous calendar month**. A month is only uploaded once it is final: all weeks are submitted **and approved** — a week that is only submitted still waits, because the PDF's total row counts only approved hours, and uploading a merely-submitted month would archive too few. Late submitters and pending approvals are caught up automatically on the next daily check. If that month still contains a pending absence request, the PDF waits until that request is approved or rejected. Employees who were archived or had time tracking disabled after the period ended are included for archive correctness when the month still contains historical data.

If the feature was disabled for several months and is then re-enabled, or if the server missed a month boundary, Zerf automatically backfills all intervening months so no timesheet is silently skipped. **Upload now** has the same backfill behaviour.

If a past month is changed after the PDF was already uploaded - for example an entry is approved or rejected, an approved entry is corrected by an admin, a week is reopened and re-submitted, or an absence is approved, cancelled, or revoked - Zerf automatically re-queues the affected employee's timesheet for that month. If a start-date change would make a re-upload hide part of that month, if the user's current start date would hide stored rows in that month, or if an archived/tracking-disabled user still has draft, submitted, or unresolved rejected entries, the upload waits and admins who opted in to technical error notifications are alerted in the app and by email. Older months are uploaded on the next daily run once they are ready. The just-finished previous month waits until the configured upload day unless an admin clicks **Upload now**.

| Setting | Description |
| --- | --- |
| Enable report PDF upload | Activates the monthly automatic upload. |
| Share link | A Nextcloud public share URL. Only `https` links are accepted. |
| Share password | Optional password protecting the share. |
| Upload day of month (1–28) | The day of the month on which the previous month's PDFs are queued. Default: 5. Set this after your team's submission deadline to maximise how many employees are already submitted when the queue is first processed. |

**Upload now** queues the previous month's PDFs for all employees immediately and uploads those whose month is already final. Employees who are not yet ready are uploaded on subsequent daily checks. This does not prevent the scheduled monthly run from processing remaining entries.

If an upload fails or a queued PDF cannot be safely generated yet, admins who opted in to technical error notifications are alerted in the app and by email. The scheduled upload retries automatically on the next daily check.

### Payroll Report

Zerf can email a monthly overview to the people who run your payroll — typically
a tax office or payroll accountant. The report replaces a hand-maintained
spreadsheet: it lists the absence days that change what has to be filed or paid,
and the working days and hours the people paid by the hour actually worked.

The report covers the **previous calendar month** and is sent as a PDF
attachment to every configured recipient — all of them receive the same email,
with no primary recipient or copy distinction. It uses the email server
configured under Settings → Email, so email must be set up first.

| Setting | Description |
| --- | --- |
| Send the payroll report by email | Activates the monthly report. |
| Recipient email addresses | One or more addresses the report is sent to, one address per line. |
| Send day of month (1–28) | The day on which the previous month is prepared. Default: 5. Set it after your submission deadline so most months are already complete on the first attempt. |
| Absence days per employee | Which categories appear is decided automatically — there is nothing to tick. Sick-like categories and any category marked **Unpaid** are included, because those are exactly the days that change what payroll has to file: sick days are needed for health-insurance reimbursement, unpaid days reduce the salary payout. Categories that don't affect pay — such as paid special leave or paid training — are left out even if they don't count as vacation or flextime either. The categories currently included are listed for reference. To change what appears, mark or unmark categories as Unpaid on the [Managing categories](#managing-categories) page. |
| Working days and hours | Tick whose working days and hours are listed: assistants, all other employees, or both. |
| People included | Everyone is included by default. Tick people under **except** to leave them out. |

At least one recipient is required before the report can be switched on, and
the report must have at least one section with content (an absence category
that qualifies, or working hours).

#### Choosing who is included

By default the report covers every employee. An assistant is included only if
they recorded at least one time entry in the reported month. An assistant with
no booked hours is irrelevant for that month: they do not appear in the PDF or
dashboard status and cannot hold up delivery. Under **People included** you can
tick individual people under **except** - they are then left out of the report
entirely and no longer hold up its delivery.

Administrators never appear in the payroll report and are not offered in the
list. Deactivated and deleted accounts are not shown either.

#### What the PDF contains

- **Absence days**: one row per absence period — person, category, first and last
  day within the reported month, and the number of contract working days it
  covers. Weekends, public holidays, and days before a person's start date are
  not counted; a period that covers only such days is left out entirely.
  A row that started in the previous month or continues into the next one is cut
  to the reported month.
- **Working days and hours**: one row per person — the number of days with
  approved working time and the total approved hours, given both as hours:minutes
  and as a decimal value for payroll. Automatic break deduction is already
  applied, exactly as on the timesheet PDF. Assistants without any time entry in
  the month are omitted.

#### When it is sent

The schedule works like the Nextcloud timesheet export. On the configured day the
previous month is prepared; if the month is not final yet the report waits and is
retried every day until it can be sent. A month counts as final when, for
everyone it covers:

- all elapsed weeks are submitted,
- no absence request is still undecided, and
- for everyone whose hours are in the report, all time entries of that month are
  approved — payroll pays by those hours, so a month that is only submitted would
  understate them, and
- no stored entries or absences lie before a person's start date, because those
  days are hidden from every report and would make the figures too low.

A month that is still open is a normal situation, not a fault — nobody is
emailed about it. Instead, the **Payroll Report** card on the dashboard shows
team leads and admins how far the month is, and who is still missing. If the
feature was switched off for a while, or the server missed a month boundary,
all intervening months are prepared as soon as it is switched on again.

**Send now** sends the previous month immediately, without waiting for everyone.
It includes everybody who has already finished their month and marks the report
clearly as **provisional**: the PDF and the email both state how many of the
people it covers and name those who are missing, together with the reason. This
way an early copy can never be mistaken for the final figures. If nobody has
finished the month yet, nothing is sent — an empty report helps nobody.

**Send now** does not replace the scheduled monthly run: the month is not marked
as delivered, so the complete report still goes out automatically on the
configured day.

#### The dashboard card

Team leads and admins see a **Payroll Report** card next to **Who is absent** on
their dashboard while the previous month's report is still outstanding. It
shows a ring split into three colours and the summary **"X of Y done"**. The
ring and detail list count only people relevant to that month; assistants
without any time entry are omitted:

| Colour | Meaning |
| --- | --- |
| Green | Everything submitted and approved — this person is done. |
| Amber | Everything submitted, but an approval or an absence decision is still missing. |
| Red | Weeks are still missing, or the data needs an administrator's attention. |

Clicking the card opens the full list, with the people who are still missing at
the top. Clicking a person opens their report for that month, so you can go
straight to what needs approving.

Team leads see the counts and colours for **everyone**, so they can tell whether
the report as a whole is ready. People outside their own team are shown as
*Not visible to you* — they still count towards the totals, but their names are
not revealed.

Once the report has been sent, the card greys out and reads e.g. *"June sent"*
for the rest of the month. The card is hidden entirely when the payroll report
is switched off.

### Managing categories

#### Time categories

Categories define what employees can book time against.

- Each category has a name and a crediting flag.
- Inactive categories are hidden from time entry forms but remain visible to
  admins for maintenance.
- A category must be active to be used in a new time entry.
- Deleting a category with existing time entries is not possible; deactivate
  instead.
- Once a category has at least one time entry (any user, any status), the
  **crediting flag** is locked. Changing it would retroactively rewrite every
  user's flextime and overtime history. To change a flag, deactivate the
  existing category and create a new one with the desired setting.
- Each category can also be enabled or disabled per employee: both the
  creation and edit dialogs show a table of all employees with a checkbox per
  row. Only checked employees can see and use the category in the
  time-entry form. The table is pre-checked for every employee by default, so
  deselecting someone is the only action needed to restrict access; new
  employees default to every existing category. Disabling a category for an
  employee only blocks *new* entries — their existing time entries in that
  category are unaffected, and reports/exports are unchanged.

#### Absence categories

Absence categories define what types of absences employees can request. Each category has three behavior fields:

| Field | Effect |
| --- | --- |
| **Cost type** | A single 3-state field that determines the balance impact of approved days. `none` — no balance impact (e.g. unpaid leave, general absence): the day is removed from the daily work target but neither leave account nor flextime is debited. `vacation` — creates a separate leave account for this category and deducts from that account, using its own per-user entitlements, carryover, and expiry. `flextime` — keeps the daily work target intact so the absence costs flextime balance. The flextime balance is checked at BOTH request and approval time against the configured floor (default 0 minutes; admin can override via the `flextime_min_balance_min` setting); the check accounts for other already-pending/approved flextime-cost absences so multiple requests that each individually fit cannot together breach the floor, and the approver's re-check catches the case where the user spent balance between request and approval. A `none` category can be a paid day off (special leave, paid training) or an unpaid one — that distinction is what the **Unpaid** field below is for. |
| **Auto-approve past dates** | Absences with a start date on or before today are approved automatically. Approvers receive an informational notice, in-app and by email. This flag also disables the time-entry conflict check at creation, so partial-day overlaps are allowed (e.g. employee worked the morning and then called in sick). Auto-approved absences that start today may extend at most 60 days into the future; longer ongoing absences require a new submission. |
| **Unpaid** | Only available when cost type is `none`. Marks days in this category as actually reducing the employee's salary — as opposed to a `none`-cost category that is still fully paid, such as special leave or paid training. This is what drives which categories show up automatically in the monthly [Payroll Report](#payroll-report): sick-like categories and anything marked Unpaid. Unlike Cost type and Auto-approve past dates, this field is not locked once the category has existing absences — it can be changed at any time. |

Constraints:
- A category slug is auto-generated from the name and must be unique. Existing absences are not affected when a category is deactivated or renamed.
- Inactive categories are hidden from the absence request dialog but remain attached to existing absence records.
- A category with cost type `vacation` has leave-account settings: default days
  for newly created users, a carryover expiry month/day, and an internal start
  year. The start year stops a newly introduced account from generating
  entitlements or carryover for older years. The values are independent for
  every leave-account category.
- Changing the cost type of an absence (e.g. from a vacation category to a flextime category) after submission is not allowed. Cancel the existing request and re-submit with the correct category.
- Once a category has at least one referencing absence (any status), the **Cost type** and **Auto-approve past dates** fields are locked. Toggling them would retroactively change the financial or approval meaning of existing rows — past balance recomputations would suddenly debit or credit different ledgers and approval workflow guards would relax or tighten without the affected employees seeing it. To change a field, deactivate the existing category and create a new one with the desired settings. Cosmetic changes (name, color, sort order, active flag) are always allowed.
- **Cost type `vacation` and Auto-approve past dates cannot both be enabled on the same category.** Setting both would let employees bypass approver review for leave-account deductions and would cause account days to appear in both a leave-account and the sick-days report columns. Use separate categories: one with `vacation` cost type (requires approval) and one with auto-approve enabled (cost type `none` or `flextime`).
- **Unpaid can only be enabled together with cost type `none`.** Leave-account and flextime categories are always paid through their own balance mechanics.
- Like time categories, each absence category can be enabled or disabled per
  employee from the same creation and edit dialogs. Only checked employees can
  request the category going forward; existing absences already in that
  category are unaffected. If an employee still has a live absence in a
  category that is later disabled for them, Zerf keeps that category's
  behavior for the existing absence but does not offer it for new requests.

### Managing holidays

Holidays define public holidays that are excluded from:

- Absence effective-workday checks (a date range spanning only holidays
  contains no effective workday).
- Submission-completeness checks (holidays are not required workdays).
- Submission-reminder unsubmitted-week detection.

Holidays are date-scoped. Load holiday data for the current and next year to
ensure all checks work correctly into the near future.
Auto-imported holidays are compared with the configured country/region source
when Zerf ensures a year, so missing or changed imported holidays are refreshed
without removing manually added holidays.

A manually added holiday can be marked to repeat every year, optionally with
a last year it still applies. This is useful for days your organization
treats as time off that aren't official public holidays — for example,
Christmas Eve or New Year's Eve — so you don't have to re-add them every
year. Deleting a repeating holiday removes it for every year, not just the
one you were viewing.

### Backup and restore

Scheduled backups capture a full snapshot of the database. Each backup is stored as a single zip archive:

- `zerf-<ts>.zip` — contains all backup data in one file with these entries:
  - `dump.enc` — AES-256-CBC encrypted PostgreSQL custom-format dump (the data you restore from).
  - `metadata` — plaintext record with the backup timestamp and git commit, used to match a backup to a specific app version.
  - `keyring.enc` — copy of the encrypted pg_tde keyring, for physical volume recovery only. Not needed for a normal restore. Included only when the keyring volume is mounted in the backup container.

#### Restoring a backup

`unzip` must be available on the machine running `restore.sh` (`apt-get install unzip`). Then run the script from the server that has Docker access to the stack:

```bash
./scripts/restore.sh
```

The script:
1. Lists the available backup archives in the backup volume (newest first) and prompts you to choose one.
2. Extracts the encrypted dump from the archive and validates that it decrypts to a valid pg_dump archive.
3. Stops the app container and the backup container to prevent writes during restore.
4. Drops all non-extension objects in the database so that a backup from an older schema version restores cleanly even if the live schema is newer.
5. Restores all data and stops on the first error (the transaction is rolled back if anything fails).
6. Restarts the backup container, then asks whether to restart the app.

Enter the listed number exactly as shown, without leading zeroes. Invalid and out-of-range choices are rejected before the restore begins.

You can also supply the archive path directly to skip the interactive listing:

```bash
./scripts/restore.sh /path/to/zerf-<ts>.zip
```

Legacy backups created before this format change (individual `.dump.enc` files) are still listed and fully restorable.

**Migration compatibility:**
- Backup older than current code: the app applies pending database migrations automatically on startup.
- Backup newer than current code: update the app binary before restarting it, or the app may not understand the restored schema.

**Size limit:** The decrypted dump is staged inside the postgres container's `/tmp` tmpfs (256 MiB by default). If your dump exceeds that, increase the `size` of the `/tmp` tmpfs in `docker-compose-local.yml` and restart the postgres container before restoring.

#### Physical recovery (corrupted or orphaned data volume)

If the postgres data volume is lost or unreadable but you have both a backup archive and the `ZERF_DB_ENCRYPTION_KEY`, restore normally with `scripts/restore.sh`.

If you have an orphaned, encrypted PGDATA volume but the pg_tde keyring volume was lost, extract the keyring from a backup archive:

```bash
./scripts/restore.sh --keyring /tmp/keyring-out
```

This extracts the `keyring.enc` entry from the selected backup archive to `/tmp/keyring-out` (as `zerf-<ts>.keyring.enc`) without touching the database. Place the extracted file as `pg_tde_keyring.enc` inside the postgres keyring volume (`zerf_postgres_data`), then start postgres against the existing data directory.

> **Warning:** Do not overwrite a working keyring. Only use this procedure when the keyring volume itself is gone.

---

## Status transition reference

### Time entry statuses

```
draft ──[submit]──> submitted ──[approve]──> approved
                          └──[reject]───> rejected

unresolved rejected ──[reopen approved]──> draft
submitted ──[reopen approved]──> draft
approved ──[reopen approved]──> draft
```

### Absence statuses

```
(creation)
  sick (start ≤ today) ──> approved (auto)
  other / future sick  ──> requested

requested ──[approve]──────────────> approved
requested ──[reject]───────────────> rejected
requested ──[cancel by employee]───> cancelled

approved  ──[cancel by employee]───> cancellation_pending
                                          ├─[approve cancellation]──> cancelled
                                          └─[reject cancellation]───> approved
approved  ──[revoke by admin]───────> cancelled
```

### Reopen request statuses

```
(creation)
  all reopenable entries are submitted --> auto_approved (week immediately reopened, silent)
  reopen auto-approval enabled         --> auto_approved (week immediately reopened, silent)
  otherwise                            --> pending

pending --[approve]--> approved
pending --[reject]--> rejected
```

`approved`, `auto_approved`, and `rejected` are terminal for the request itself
(the week's *entries* are what move back to `draft` when the reopen executes —
see [Time entry statuses](#time-entry-statuses) above).

## Security and access control

### Authentication

- Sessions use SHA-256 hashed tokens stored server-side with absolute (168 h)
  and idle (8 h) timeouts enforced at the middleware level.
- Session cookies are `HttpOnly`, `SameSite=Strict`, and `Secure` (when
  configured) to prevent XSS and CSRF token theft.
- CSRF protection uses a double-submit pattern: every state-changing request
  must include an `X-CSRF-Token` header that matches the server-stored session
  token. Origin/Referer headers are additionally validated.
- Login is rate-limited: after 5 failed attempts within 15 minutes the account
  is temporarily locked. Generic error messages prevent email enumeration.
- Password reset tokens are single-use, SHA-256 hashed, and expire after 1 h.
  Inactive accounts cannot request password resets.

### Temporary passwords and forced password change

- When an admin resets a user's password, the system issues a one-time
  temporary credential, marks the account with `must_change_password`, and
  sends the new password to the user via email (when SMTP is configured).
- Until the password is changed, the backend middleware blocks **all** API
  endpoints except `/auth/me`, `/auth/password`, `/auth/logout`,
  `/auth/preferences`, and `/settings/public`. This prevents temporary
  credentials from being used to access or modify any sensitive data.
- The frontend enforces the same restriction via route-level redirects.

### Role-based access control

- **Admin** – full access to all users, settings, and data.
- **Team lead** – can view/approve only for users explicitly assigned to them
  via the approver chain. Cannot view or act on admin-subject data.
- **Employee** – can only access their own time entries, absences, and reports.
- **Assistant** – same as employee but excluded from flextime/overtime and
  dashboard.

Non-admin team leads are prevented from:

- Viewing or approving time entries / absences for admin-role users.
- Accessing users not in their direct report list.
- Approving their own submissions (self-approval prevention).

**Scoped assistant management endpoints (`/team-users*`):** when a non-admin
team lead is granted this capability (see [Scoped assistant user management
(optional)](#scoped-assistant-user-management-optional)), every request is
re-validated server-side, independent of what the client sends:

- The admin setting must be enabled, or every `/team-users*` request is
  rejected.
- List results only ever include the requester's own assigned users
  (active or not); for anyone who is not an "Assistant" the server omits
  every field except the name — there is no payload to leak even if the
  frontend had a bug.
- Create always forces role `assistant` and approver = the requesting lead,
  ignoring any role or approver value sent by the client.
- Get/update require the target to be both a direct report of the requester
  (active or not) **and** role `assistant`; anyone else (including a
  different lead's assistant, or the requester's own account) is rejected
  with `403 Forbidden`.
- There is no delete route under `/team-users*` at all — a non-admin lead
  can archive and restore an assigned assistant via the dedicated
  `/team-users/{id}/archive` and `/team-users/{id}/restore` endpoints,
  but can never delete one.
- Admins are unaffected and always use the regular `/users*` endpoints.

### Pure-admin mode (tracks_time=false)

- Admins with `tracks_time=false` cannot create, view, or export their own
  time entries, absences, or reports.
- They retain full access to team management, approval workflows, the calendar,
  and the Settings panel.
- The navigation bar automatically hides Time and Absences links for these
  accounts.
- The Calendar remains accessible since pure-admins need team schedule
  visibility for coordination.
- Report endpoints return Forbidden for targets with `tracks_time=false` or
  inactive status.
- **Disabling time tracking preserves all existing data.** Time entries,
  absences, and reopen requests are never deleted when `tracks_time` is set to
  `false`. The rows are retained immutably in the database but are silently
  excluded from all team views, approval queues, reminder notifications, and
  calculations. If automatic report PDF upload is enabled, months that already
  contain historical data can still be uploaded for archive completeness.

### Session invalidation

Sessions are automatically destroyed when:

- A user's role is changed.
- A user is archived or deleted (cascade on delete).
- A user changes their password (all other sessions are killed).
- An admin resets a user's password.
- A user logs out (all sessions for that user are killed).

### Audit trail

All significant administrative and approval actions are logged to the audit
table, including:

- User creation, update, archive, restore, and deletion.
- Password resets.
- Time entry status transitions (submit, approve, reject, reopen, and silent
  auto-approval on submit), recorded once per week and employee.
- Absence creation, approval, rejection, revocation, and cancellation.
- Admin settings changes (language, timezone, country, region).
- SMTP configuration changes.
- Team settings modifications (allow_reopen_without_approval,
  allow_submission_without_approval).
- Category creation, update, and deletion.
- Holiday creation, update, and deletion.

### Input validation and DoS prevention

- Date range queries are limited to a maximum of 366 days.
- Year parameters are bounded to 1970–2100.
- Batch operations (submit, approve, reject) are limited to 500 entries.
- Status filter parameters are validated against known values.
- Comments are limited to 2 000 characters.
- CSV exports include formula-injection guards (leading `=`, `+`, `-`, `@`,
  tab, or CR are prefixed with a single-quote).

### Information disclosure prevention

- Password hashes are never serialized in API responses.
- SMTP passwords are never returned; only a boolean `smtp_password_set`
  indicates whether one is configured.
- Absence and calendar visibility has no per-category masking (see
  [Overlap rules](#overlap-rules) above): if your scope doesn't cover a user
  at all, you see nothing about them; if it does (your own data, your direct
  report's, or any user's as an admin), you see the real kind, comment, and
  category — never a redacted placeholder.
