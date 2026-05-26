# Frontend Refactoring Plan

## Context

`Reports.svelte` (1,818 lines) and `Dashboard.svelte` (1,313 lines) are monoliths containing
multiple independent sections that should be separate components. The `routes/time/` directory
already demonstrates the correct pattern — `Time.svelte` (301 lines) delegates to `DayCard`,
`EntryBlock`, `WeekGrid`, etc. This plan applies the same pattern to Reports and Dashboard.

A parallel problem: `lib/ui/` contains 8 shared primitives (`SectionCard`, `StatCard`,
`DataTable`, `LoadingState`, `EmptyState`, `HelpToggle`, `FormField`, `DateRangeFields`) that
were created but never adopted — the two biggest files repeat their patterns inline.

The goal is to bring both files down to orchestration-only code (~200–300 lines each) while
making the shared UI library actually useful.

---

## Files Modified

### New files created
```
frontend/src/lib/exports/reportPdf.js              ← extracted from Reports.svelte lines 641–878
frontend/src/lib/domain/calendar.js               ← 10 pure functions extracted from Calendar.svelte
frontend/src/lib/domain/auditLog.js               ← 11 pure functions extracted from AdminAuditLog.svelte
frontend/src/routes/reports/EmployeeReport.svelte
frontend/src/routes/reports/TeamReport.svelte
frontend/src/routes/reports/CategoryReport.svelte
frontend/src/routes/reports/AbsenceReport.svelte
frontend/src/routes/reports/TimesheetExport.svelte
frontend/src/routes/dashboard/AbsenceSlider.svelte
frontend/src/dialogs/AbsenceDetailDialog.svelte    ← replaces inline dialog in Absences.svelte
frontend/src/dialogs/AbsenceReviewDialog.svelte    ← replaces inline dialog, Dashboard lines 1044–1136
frontend/src/dialogs/ReopenReviewDialog.svelte     ← replaces inline dialog, Dashboard lines 1138–1192
frontend/src/dialogs/WeekReviewDialog.svelte       ← replaces inline dialog, Dashboard lines 1194–1233
```

### Files shrunk significantly
```
frontend/src/routes/Reports.svelte    1824 → ~200 lines (user-loading + layout only)
frontend/src/routes/Dashboard.svelte  1315 → ~500 lines (state + approval actions; dialogs extracted)
frontend/src/routes/Calendar.svelte    440 → ~150 lines (load + clickDay only)
frontend/src/routes/AdminAuditLog.svelte 515 → ~120 lines (load + openDetail only)
frontend/src/routes/Absences.svelte    572 → ~530 lines (detail dialog extracted)
```

### Unchanged
All existing `lib/domain/`, `lib/api/`, `dialogs/AbsenceDialog.svelte`,
`dialogs/EntryDialog.svelte`, tests, backend.

---

## Phase 1 — Extract PDF generation (pure JS, zero UI risk)

**Why first:** no Svelte involved; can be tested in isolation; unblocks TimesheetExport
extraction in Phase 2.

### `frontend/src/lib/exports/reportPdf.js`

Move the five inner functions and `exportPdf` body out of `Reports.svelte`:

```js
// Public API
export function buildReportPdf(reportData, userName, settings, t) → Blob
```

The function receives the already-fetched data objects and returns a PDF `Blob`. It has no
dependency on Svelte stores or component state — only `jsPDF` and formatting helpers.

Inner helpers (`colX`, `textX`, `drawHeader`, `drawRow`, `drawSummaryRow`) become module-private.

**Callers:** `TimesheetExport.svelte` imports `buildReportPdf` and calls `downloadBlob` locally.

---

## Phase 2 — Decompose Reports.svelte

Follow the `routes/time/` convention: create a `routes/reports/` subdirectory.

### What stays in `Reports.svelte` after decomposition

```svelte
<script>
  // One-time users fetch (shared by all cards that have a user dropdown)
  let users = [];
  async function initUsers() { … }
  initUsers();

  // Derived booleans passed as props
  $: canViewTeamReports = !!$currentUser?.permissions?.can_view_team_reports;
  $: isSelfOnlyReportsView = !canViewTeamReports;
</script>

<!-- Layout: five card components in order -->
<EmployeeReport {users} {isSelfOnlyReportsView} />
{#if canViewTeamReports}<TeamReport />{/if}
<CategoryReport {users} {isSelfOnlyReportsView} />
<AbsenceReport {users} {isSelfOnlyReportsView} />
<TimesheetExport {users} {isSelfOnlyReportsView} />
```

### `routes/reports/EmployeeReport.svelte`

**Props:** `users`, `isSelfOnlyReportsView`  
**Stores accessed directly:** `$currentUser`, `$settings`, `$earliestStartDate`, `$toast`

**Owns:**
- `reportUserId`, `reportMonth`, `reportData`, `activeHelp`
- Reactive: `selectedReportUser`, `selectedUserIsAssistant`, `selectedUserHasFlextime`,
  `reportMinMonth`, `reportAbsenceSummary`
- Functions: `loadReport`, `userHasFlextime`, `userWorkdaysPerWeek`

**Template:** the current Card 1 block (lines 895–1263), including the employee dropdown, month
picker, stat cards, entries table, absences table, leave balance, and flextime chart.

### `routes/reports/TeamReport.svelte`

**Props:** none  
**Stores:** `$currentUser`, `$earliestStartDate`

**Owns:**
- `teamMonth`, `teamReport`, `activeHelp`
- Reactive: `earliestStartMonth` (re-derived from `$earliestStartDate` directly)
- Function: `showTeam`

**Template:** Card 3 block (lines 1266–1413).

### `routes/reports/CategoryReport.svelte`

**Props:** `users`, `isSelfOnlyReportsView`  
**Stores:** `$currentUser`

**Owns:**
- `catUserId`, `catMonth`, `catReport`, `teamCatReport`, `selectedCategories`, `activeHelp`
- Reactive: derived category columns, filter state
- Functions: `showCat`, `toggleCategoryFilter`, `teamCatMinutes`, `teamCatRowTotal`

**Template:** Card 4 block (lines 1414–1626).

### `routes/reports/AbsenceReport.svelte`

**Props:** `users`, `isSelfOnlyReportsView`  
**Stores:** `$currentUser`, `$settings`

**Owns:**
- `absenceUserId`, `absenceFrom`, `absenceTo`, `absenceReport`, `activeHelp`
- Functions: `showAbsences`, `clampAbsenceRange`, `absenceDays`, `loadOwnAbsencesForRange`

**Template:** Card 5 block (lines 1627–1734).

### `routes/reports/TimesheetExport.svelte`

**Props:** `users`, `isSelfOnlyReportsView`  
**Stores:** `$currentUser`, `$settings`

**Owns:**
- `csvUserId`, `csvYear`, `csvFrom`, `csvTo`, `activeHelp`
- Reactive: `csvUserMinDate`
- Functions: `exportCsv`, `exportPdf` (delegates to `reportPdf.js`), `csvSafe`, `csvEncode`,
  `downloadBlob`, `userHasFlextime`, `userWorkdaysPerWeek`

**Template:** Card 6 block (lines 1735–1818).

---

## Phase 3 — Decompose Dashboard.svelte

### 3a. Extract the four inline dialogs

These are self-contained. They receive data and fire callbacks; no shared mutable state.

#### `dialogs/AbsenceDetailDialog.svelte`  ← extracted from `Absences.svelte`

```svelte
<script>
  export let absence;   // the absence object (read-only display)
  export let onClose;
  export let onCancel;  // (absence) => void — only rendered when absence.cancellable
</script>
```

Contains the current detail dialog in `Absences.svelte` (lines ~363–420). Shows dates, status,
comment, rejection reason, and optionally a "Cancel" button. The existing `dialogs/AbsenceDialog.svelte`
handles *requesting* an absence; this handles *viewing* one.

#### `dialogs/AbsenceReviewDialog.svelte`  ← extracted from `Dashboard.svelte`

```svelte
<script>
  export let absence;    // the absence object
  export let users;      // for userName lookup
  export let onClose;
  export let onApprove;  // (absence) => void
  export let onReject;   // (absence) => void
</script>
```

Contains the current absence detail dialog markup (lines 1044–1136) including the change-diff
table. The `absenceDiffRows` helper moves here (or to `lib/domain/dashboard.js`).

**Naming note:** `dialogs/AbsenceDialog.svelte` already exists and is the *request* dialog.
This is the *review* dialog, hence the different name.

#### `dialogs/ReopenReviewDialog.svelte`

```svelte
<script>
  export let item;
  export let users;
  export let onClose;
  export let onApprove;  // (id) => void
  export let onReject;   // (id) => void
</script>
```

Contains lines 1138–1192.

#### `dialogs/WeekReviewDialog.svelte`

```svelte
<script>
  export let week;
  export let users;
  export let busy = false;
  export let onClose;
  export let onApprove;  // (week) => void
  export let onReject;   // (week) => void
</script>
```

Contains lines 1194–1233.

**Usage in Dashboard.svelte after extraction:**
```svelte
<AbsenceReviewDialog
  absence={absenceDetail}
  {users}
  onClose={() => (absenceDetail = null)}
  onApprove={approveAbsence}
  onReject={rejectAbsence}
/>
```

### 3b. Extract absence slider

#### `routes/dashboard/AbsenceSlider.svelte`

**Props:** `users` (for the name lookup)  
**Owns all slider state internally:** `week`, `data`, `direction`, loading

```svelte
<script>
  export let users = [];
</script>
```

The parent Dashboard no longer needs `absenceSliderWeek`, `absenceSliderTeamData`,
`absenceSliderDirection`, `absenceSliderIsLeadView`, or the three slider functions.

### 3c. Remove trivial wrapper functions

These three functions in Dashboard.svelte delegate to an import with no added logic:

```js
// Remove — call the imported functions directly in the template:
function userName(userId, userRows)     → userNameFromRows(userId, userRows)
function userInitials(userId, userRows) → userInitialsFromRows(userId, userRows) || "?"
function hoursFromMinutes(minutes)      → formatHours((minutes || 0) / 60)
```

Update all call sites in the template to use the imports directly.

### 3d. Consolidate `rejectAbsence` duplicate branches

The two near-identical `try/catch` blocks inside `rejectAbsence` can be merged:

```js
async function rejectAbsence(absence) {
  const isCancellation = absence.status === "cancellation_pending";
  const result = await confirmDialog(
    isCancellation ? $t("Reject cancellation?") : $t("Reject?"),
    isCancellation
      ? $t("Reject this cancellation request? The absence will remain approved.")
      : $t("Reject this request?"),
    { danger: true, confirm: $t("Reject"), reason: !isCancellation },
  );
  if (!result) return;
  try {
    await rejectAbsenceById(absence, isCancellation ? undefined : result);
    toast($t("Rejected."), "ok");
    load();
  } catch (error) {
    toast($t(error?.message || "Error"), "error");
  }
}
```

---

## Phase 4 — Adopt `lib/ui/` components

Apply progressively inside the new sub-components (not retroactively to unchanged files).

### Priority targets

| Component | Replaces | Where |
|---|---|---|
| `<StatCard>` | `div.zf-card.stat-card > div.stat-card-label + div.stat-card-value` | EmployeeReport, Dashboard stat sections |
| `<DataTable>` | `div.zf-table-wrap > table.zf-table` | All 6 raw `<table>` blocks in Reports cards |
| `<SectionCard>` | `div.zf-card` with title + help pattern | Card wrappers across new components |
| `<LoadingState>` | Inline `{#if loading}…` skeleton patterns | Where loading states exist |
| `<EmptyState>` | Inline "no data" `div` patterns | Where empty state text appears |

`StatCard.svelte` has `label`, `value`, and an unnamed slot for sub-text — covers all inline
patterns without modification.

`SectionCard.svelte` has `title`, `helpText`, `helpOpen`, `onHelpToggle`, and an `actions`
slot — covers the card header + help toggle pattern. `HelpToggle` is already imported by it.

---

---

## Files assessed — no changes needed

Every Svelte file not listed above was reviewed and found appropriately structured.

| File | Lines | Verdict |
|---|---|---|
| `App.svelte` | 407 | Router. Script-heavy by nature; `preferredHome`, `canAccessRoute`, `resolveRoute` are routing helpers that belong here. Fine. |
| `AppLogo.svelte` | 50 | Static logo component. Fine. |
| `Confirm.svelte` | 78 | Thin confirm-dialog wrapper. Fine. |
| `DatePicker.svelte` | 566 | Complex calendar widget with keyboard/touch; script size is justified. Fine. |
| `Dialog.svelte` | 45 | Generic modal shell. Fine. |
| `FlextimeChart.svelte` | 520 | Chart renderer; helpers (`absColor`, `dayBandColor`, `fmtBal`) are chart-internal and stay here. Fine. |
| `Icons.svelte` | 62 | SVG icon registry. Fine. |
| `Layout.svelte` | 490 | App shell (nav, notifications, mobile bar). All 8 functions are UI-coupled. Fine. |
| `TimePicker.svelte` | 459 | Complex picker with mouse/wheel/touch/keyboard; 20 functions are all interaction handlers. Fine. |
| `dialogs/AbsenceDialog.svelte` | 185 | Absence *request* dialog. Single-concern. Fine. |
| `dialogs/CategoryDialog.svelte` | 102 | Category create/edit. Fine. |
| `dialogs/EntryDialog.svelte` | 183 | Time-entry create/edit. Fine. |
| `dialogs/UserDialog.svelte` | 517 | Complex create/edit form with role-conditional fields; password generator helpers are private to this dialog. Fine. |
| `lib/ui/CategoryDot.svelte` | 15 | Tiny color dot. Fine. |
| `lib/ui/DateRangeFields.svelte` | 22 | Two date inputs. Fine. |
| `lib/ui/FormField.svelte` | 11 | Label + slot wrapper. Fine. |
| `lib/ui/HelpToggle.svelte` | 18 | Info button. Fine. Already used by SectionCard. |
| `lib/ui/PageHeader.svelte` | 18 | Page title wrapper. Fine. |
| `lib/ui/PullToRefresh.svelte` | 102 | Pull-to-refresh gesture handler. Fine. Already used by Layout. |
| `lib/ui/StatGrid.svelte` | 3 | Three-line grid wrapper. Fine. Already used by TimeWeekSummary. |
| `lib/ui/StatusChip.svelte` | 7 | Status badge. Fine. Already used by EntryBlock. |
| `routes/Absences.svelte` | 572 | Absence list for employees. 7 UI-coupled functions. 131-line `<style>` is page-specific. The inline detail dialog is extracted in Phase 3a above. Remaining structure fine. |
| `routes/Account.svelte` | 299 | Personal account settings form. Fine. |
| `routes/AdminCategories.svelte` | 65 | Category list. Fine. |
| `routes/AdminEmail.svelte` | 299 | Email settings form. Fine. |
| `routes/AdminHolidays.svelte` | 140 | Holiday management. Fine. |
| `routes/AdminSettings.svelte` | 442 | App settings form with country/region loading. Fine. |
| `routes/AdminTabs.svelte` | 39 | Tab navigation bar. Fine. |
| `routes/AdminUsers.svelte` | 169 | User list with inline actions. Fine. |
| `routes/Login.svelte` | 357 | Login / forgot / reset flows. Three related flows in one file is standard practice. Fine. |
| `routes/NotFound.svelte` | 13 | 404 page. Fine. |
| `routes/Setup.svelte` | 202 | First-run setup wizard. Fine. |
| `routes/TeamSettings.svelte` | 129 | Team config form. Fine. |
| `routes/Time.svelte` | 301 | Already decomposed into `routes/time/` sub-components. The exemplar this plan follows. |
| `routes/time/DayCard.svelte` | 144 | Day column card. Fine. |
| `routes/time/EntryBlock.svelte` | 92 | Single time entry row. Fine. Uses `CategoryDot`, `StatusChip`. |
| `routes/time/TimeWeekHeader.svelte` | 81 | Week header. Fine. |
| `routes/time/TimeWeekSummary.svelte` | 43 | Week totals. Fine. Uses `StatCard`, `StatGrid`. |
| `routes/time/WeekGrid.svelte` | 33 | Week layout grid. Fine. |
| `routes/time/WeekendEntries.svelte` | 29 | Weekend entry display. Fine. |

---

## Phase 5 — Extract Calendar and AdminAuditLog domain logic

Both files follow the same pattern as `lib/domain/dashboard.js`: heavy pure-function script
blocks that belong in testable modules, leaving the Svelte file as a thin UI shell.

### `lib/domain/calendar.js`

Move these 11 functions out of `Calendar.svelte` (currently lines ~109–237):

```js
export function absColor(kind)
export function normalizeColor(color)
export function fallbackColor(offset, used)
export function categoryForEntry(entry, categoryMap)
export function workLabel(entry, categoryMap)
export function workBaseColor(entry, offset, categoryMap)
export function absenceDetail(absence)
export function rawCellEvents(cell, entryMap, categoryMap, translate, userMap, currentUserId)
export function buildColorMap(baseCells, entryMap, categoryMap, translate)
export function cellEvents(cell, entryMap, categoryMap, colorMap, translate, userMap, currentUserId)
export function calendarEventTitle(event)
```

All are pure (no DOM, no stores). Only `load()` and `clickDay()` remain in the component.
Script block shrinks from **332 → ~80 lines**.

### `lib/domain/auditLog.js`

Move these 11 functions out of `AdminAuditLog.svelte` (currently lines ~28–267):

```js
export function safeParseJson(raw)
export function relevantPayload(entry)
export function weekInfoFromEntry(entry)
export function summarize(entry, translate)
export function userLabel(userId, userMap, translate)
export function subjectUserId(entry)
export function subjectUserLabel(entry, userMap)
export function fmtFieldVal(key, val, userMap, translate)
export function extractDetailRows(entry, userMap, translate)
export function actionClass(action)
export function buildRows(entries, userMap, translate)
```

All are pure. Only `load()` and `openDetail()` remain in the component.
Script block shrinks from **270 → ~30 lines**.

The 160-line `<style>` block in `AdminAuditLog.svelte` is audit-log-specific and correctly
scoped — leave it in place.

---

## Execution order

1. `lib/exports/reportPdf.js` — pure JS extraction, no UI
2. `lib/domain/calendar.js` — pure JS extraction, no UI
3. `lib/domain/auditLog.js` — pure JS extraction, no UI
4. `routes/reports/` — five sub-components; shrinks the biggest file first
5. `dialogs/AbsenceDetailDialog.svelte` — extract from Absences.svelte
6. Dashboard dialogs — three extractions (AbsenceReview, ReopenReview, WeekReview)
7. `routes/dashboard/AbsenceSlider.svelte`
8. Dashboard dead-code cleanup + `rejectAbsence` consolidation
9. `lib/ui/` adoption pass on the new components

---

## Verification

**After each phase:**
```bash
cd frontend && npx vitest run   # all 175 tests must stay green
```

**Visual smoke test (after Phase 2 and 3):**
- `/reports` as admin — all 5 cards render, load buttons work, CSV and PDF download correctly
- `/reports` as employee — only Card 1 visible, month selector works
- `/dashboard` as admin — stat cards load, approval queue renders, all three dialogs open and
  approve/reject correctly, absence slider advances weeks
- `/dashboard` as employee — personal stat cards load, no approval queue visible

**Line count targets:**
```bash
wc -l frontend/src/routes/Reports.svelte    # target: < 250
wc -l frontend/src/routes/Dashboard.svelte  # target: < 600
```
