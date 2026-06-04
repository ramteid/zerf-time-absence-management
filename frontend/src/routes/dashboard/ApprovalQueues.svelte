<script>
  import { t, formatHours, absenceKindLabel } from "../../i18n.js";
  import { fmtDateShort, fmtWeekLabel } from "../../format.js";
  import Icon from "../../Icons.svelte";
  import { absenceRequestTypeLabelKey } from "../../lib/domain/dashboard.js";
  import {
    userInitialsFromRows,
    userNameFromRows,
  } from "../../lib/domain/users.js";

  export let pendingWeeks = [];
  export let pendingReopens = [];
  export let pendingAbsences = [];
  export let users = [];
  export let focusedSection = "";
  export let timesheetsSectionEl = null;
  export let absencesSectionEl = null;
  export let onBatchApprove = () => {};
  export let onOpenWeekDetails = () => {};
  export let onApproveWeek = () => {};
  export let onRejectWeek = () => {};
  export let onOpenReopenDetail = () => {};
  export let onApproveReopen = () => {};
  export let onRejectReopen = () => {};
  export let onShowAbsenceDetail = () => {};
  export let onApproveAbsence = () => {};
  export let onRejectAbsence = () => {};
</script>

<div class="dashboard-approval-grid">
  <div
    class="zf-card"
    class:dashboard-focus={focusedSection === "timesheets"}
    style="overflow-x:auto"
    bind:this={timesheetsSectionEl}
  >
    <div class="card-header">
      <Icon name="CalendarCheck" size={15} sw={1.5} />
      <span class="card-header-title">{$t("Week Approvals")}</span>
      {#if pendingWeeks.length + pendingReopens.length > 0}
        <span class="zf-chip zf-chip-pending" style="font-size:10.5px">
          {pendingWeeks.length + pendingReopens.length}
          {$t("pending")}
        </span>
      {/if}
      {#if pendingWeeks.length}
        <button class="zf-btn zf-btn-sm" on:click={onBatchApprove}>
          <Icon name="Check" size={13} />{$t("Approve All")}
        </button>
      {/if}
    </div>

    {#each pendingWeeks as week (week.key)}
      <div
        class="dashboard-click-row"
        on:click={() => onOpenWeekDetails(week)}
        on:keydown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            onOpenWeekDetails(week);
          }
        }}
        role="button"
        tabindex="0"
        title={$t("Show")}
      >
        <div class="avatar" style="width:30px;height:30px;font-size:11px">
          {userInitialsFromRows(week.user_id, users) || "?"}
        </div>
        <div style="flex:1;min-width:0">
          <div style="font-size:13px;font-weight:500;display:flex;align-items:center;gap:6px">
            {userNameFromRows(week.user_id, users)}
            <span class="zf-chip zf-chip-submitted" style="font-size:10px">{$t("Approval")}</span>
          </div>
          <div class="tab-num" style="font-size:11.5px;color:var(--text-tertiary)">
            {fmtWeekLabel(week.week_start)} · {formatHours(week.total_min / 60)}
          </div>
        </div>
        <div style="display:flex;gap:4px">
          <button
            class="zf-btn-icon-sm"
            style="color:var(--success-text);background:var(--success-soft)"
            title={$t("Approve")}
            on:click|stopPropagation={() => onApproveWeek(week)}
          >
            <Icon name="Check" size={14} />
          </button>
          <button
            class="zf-btn-icon-sm"
            style="color:var(--danger-text);background:var(--danger-soft)"
            title={$t("Reject")}
            on:click|stopPropagation={() => onRejectWeek(week)}
          >
            <Icon name="X" size={14} />
          </button>
        </div>
      </div>
    {/each}

    {#each pendingReopens as reopen (reopen.id)}
      <div
        class="dashboard-click-row"
        on:click={() => onOpenReopenDetail(reopen)}
        on:keydown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            onOpenReopenDetail(reopen);
          }
        }}
        role="button"
        tabindex="0"
        title={$t("Show details")}
      >
        <div class="avatar" style="width:30px;height:30px;font-size:11px">
          {userInitialsFromRows(reopen.user_id, users) || "?"}
        </div>
        <div style="flex:1;min-width:0">
          <div style="font-size:13px;font-weight:500;display:flex;align-items:center;gap:6px">
            {userNameFromRows(reopen.user_id, users)}
            <span class="zf-chip zf-chip-pending" style="font-size:10px">{$t("Edit request")}</span>
          </div>
          <div class="tab-num" style="font-size:11.5px;color:var(--text-tertiary)">
            {$t("wants to edit {week_label}", { week_label: fmtWeekLabel(reopen.week_start) })}
          </div>
          {#if reopen.reason}
            <div class="reopen-reason" title={reopen.reason}>
              {reopen.reason}
            </div>
          {/if}
        </div>
        <div style="display:flex;gap:4px">
          <button
            class="zf-btn-icon-sm"
            style="color:var(--success-text);background:var(--success-soft)"
            title={$t("Approve")}
            on:click|stopPropagation={() => onApproveReopen(reopen.id)}
          >
            <Icon name="Check" size={14} />
          </button>
          <button
            class="zf-btn-icon-sm"
            style="color:var(--danger-text);background:var(--danger-soft)"
            title={$t("Reject")}
            on:click|stopPropagation={() => onRejectReopen(reopen.id)}
          >
            <Icon name="X" size={14} />
          </button>
        </div>
      </div>
    {/each}

    {#if pendingWeeks.length === 0 && pendingReopens.length === 0}
      <div class="empty-queue">
        <Icon name="Check" size={24} sw={1.2} />
        <div style="margin-top:8px">{$t("All caught up!")}</div>
      </div>
    {/if}
  </div>

  <div
    class="zf-card"
    class:dashboard-focus={focusedSection === "absences"}
    style="overflow-x:auto"
    bind:this={absencesSectionEl}
  >
    <div class="card-header">
      <Icon name="Plane" size={15} sw={1.5} />
      <span class="card-header-title">{$t("Absence Requests")}</span>
      {#if pendingAbsences.length}
        <span class="zf-chip zf-chip-pending" style="font-size:10.5px">
          {pendingAbsences.length}
          {$t("pending")}
        </span>
      {/if}
    </div>

    {#each pendingAbsences as absence (absence.id)}
      <div class="absence-row">
        <div class="avatar" style="width:30px;height:30px;font-size:11px">
          {userInitialsFromRows(absence.user_id, users) || "?"}
        </div>
        <div
          class="absence-summary"
          on:click={() => onShowAbsenceDetail(absence)}
          on:keydown={(e) => {
            if (e.key === "Enter") onShowAbsenceDetail(absence);
          }}
          role="button"
          tabindex="0"
          title={$t("Show details")}
        >
          <div style="font-size:13px;font-weight:500;display:flex;align-items:center;gap:6px">
            {userNameFromRows(absence.user_id, users)}
            <span
              class="zf-chip {absence.status === 'cancellation_pending' ? 'zf-chip-cancellation_pending' : 'zf-chip-warning'}"
              style="font-size:10px"
            >
              {$t(absenceRequestTypeLabelKey(absence))}
            </span>
          </div>
          <div class="tab-num" style="font-size:11.5px;color:var(--text-tertiary)">
            {absenceKindLabel(absence.kind)} · {fmtDateShort(absence.start_date)} -
            {fmtDateShort(absence.end_date)}
          </div>
          {#if absence.comment}
            <div class="absence-comment" title={absence.comment}>
              {absence.comment}
            </div>
          {/if}
        </div>
        <div style="display:flex;gap:4px">
          <button
            class="zf-btn-icon-sm"
            style="color:var(--success-text);background:var(--success-soft)"
            on:click={() => onApproveAbsence(absence)}
          >
            <Icon name="Check" size={14} />
          </button>
          <button
            class="zf-btn-icon-sm"
            style="color:var(--danger-text);background:var(--danger-soft)"
            on:click={() => onRejectAbsence(absence)}
          >
            <Icon name="X" size={14} />
          </button>
        </div>
      </div>
    {/each}

    {#if pendingAbsences.length === 0}
      <div class="empty-queue">
        <Icon name="Plane" size={24} sw={1.2} />
        <div style="margin-top:8px">{$t("No pending requests")}</div>
      </div>
    {/if}
  </div>
</div>

<style>
  .dashboard-approval-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 16px;
  }

  .dashboard-click-row,
  .absence-row {
    padding: 10px 16px;
    border-bottom: 1px solid var(--border);
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .dashboard-click-row {
    cursor: pointer;
  }

  .dashboard-click-row:hover {
    background: var(--bg-subtle);
  }

  .dashboard-focus {
    box-shadow: 0 0 0 2px var(--accent);
  }

  .reopen-reason,
  .absence-comment {
    font-size: 11px;
    color: var(--text-tertiary);
    margin-top: 2px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 300px;
  }

  .absence-summary {
    flex: 1;
    min-width: 0;
    cursor: pointer;
  }

  .empty-queue {
    padding: 32px;
    text-align: center;
    color: var(--text-tertiary);
    font-size: 13px;
  }
</style>
