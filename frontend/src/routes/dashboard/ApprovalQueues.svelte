<script>
  import { t, formatHours, absenceKindLabel } from "../../i18n.js";
  import { fmtDateShort, fmtWeekLabel } from "../../format.js";
  import Icon from "../../Icons.svelte";
  import { absenceRequestTypeLabelKey } from "../../lib/domain/dashboard.js";
  import {
    findUserById,
    userAvatarClass,
    userFullName,
    userInitials,
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
    class="zf-card zf-table-wrap"
    class:dashboard-focus={focusedSection === "timesheets"}
    bind:this={timesheetsSectionEl}
  >
    <div class="card-header">
      <Icon name="CalendarCheck" size={15} sw={1.5} />
      <span class="card-header-title">{$t("Week Approvals")}</span>
      {#if pendingWeeks.length + pendingReopens.length > 0}
        <span class="zf-chip zf-chip-pending zf-chip-sm">
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
      {@const weekUser = findUserById(users, week.user_id)}
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
        <div class="avatar {userAvatarClass(weekUser)} avatar-sm">
          {userInitials(weekUser) || "?"}
        </div>
        <div class="flex-min0">
          <div class="zf-row zf-item-title">
            {userFullName(weekUser, `#${week.user_id}`)}
            <span class="zf-chip zf-chip-submitted zf-chip-sm"
              >{$t("Approval")}</span
            >
          </div>
          <div class="tab-num text-hint">
            {fmtWeekLabel(week.week_start)} · {formatHours(week.total_min / 60)}
          </div>
        </div>
        <div class="zf-actions">
          <button
            class="zf-btn-icon-sm zf-btn-approve"
            title={$t("Approve")}
            on:click|stopPropagation={() => onApproveWeek(week)}
          >
            <Icon name="Check" size={14} />
          </button>
          <button
            class="zf-btn-icon-sm zf-btn-reject"
            title={$t("Reject")}
            on:click|stopPropagation={() => onRejectWeek(week)}
          >
            <Icon name="X" size={14} />
          </button>
        </div>
      </div>
    {/each}

    {#each pendingReopens as reopen (reopen.id)}
      {@const reopenUser = findUserById(users, reopen.user_id)}
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
        <div class="avatar {userAvatarClass(reopenUser)} avatar-sm">
          {userInitials(reopenUser) || "?"}
        </div>
        <div class="flex-min0">
          <div class="zf-row zf-item-title">
            {userFullName(reopenUser, `#${reopen.user_id}`)}
            <span class="zf-chip zf-chip-pending zf-chip-sm"
              >{$t("Edit request")}</span
            >
          </div>
          <div class="tab-num text-hint">
            {$t("wants to edit {week_label}", {
              week_label: fmtWeekLabel(reopen.week_start),
            })}
          </div>
          {#if reopen.reason}
            <div class="reopen-reason" title={reopen.reason}>
              {reopen.reason}
            </div>
          {/if}
        </div>
        <div class="zf-actions">
          <button
            class="zf-btn-icon-sm zf-btn-approve"
            title={$t("Approve")}
            on:click|stopPropagation={() => onApproveReopen(reopen.id)}
          >
            <Icon name="Check" size={14} />
          </button>
          <button
            class="zf-btn-icon-sm zf-btn-reject"
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
        <div class="mt-8">{$t("All caught up!")}</div>
      </div>
    {/if}
  </div>

  <div
    class="zf-card zf-table-wrap"
    class:dashboard-focus={focusedSection === "absences"}
    bind:this={absencesSectionEl}
  >
    <div class="card-header">
      <Icon name="Plane" size={15} sw={1.5} />
      <span class="card-header-title">{$t("Absence Requests")}</span>
      {#if pendingAbsences.length}
        <span class="zf-chip zf-chip-pending zf-chip-sm">
          {pendingAbsences.length}
          {$t("pending")}
        </span>
      {/if}
    </div>

    {#each pendingAbsences as absence (absence.id)}
      {@const absenceUser = findUserById(users, absence.user_id)}
      <div class="absence-row">
        <div class="avatar {userAvatarClass(absenceUser)} avatar-sm">
          {userInitials(absenceUser) || "?"}
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
          <div class="zf-row zf-item-title">
            {userFullName(absenceUser, `#${absence.user_id}`)}
            <span
              class="zf-chip zf-chip-sm {absence.status ===
              'cancellation_pending'
                ? 'zf-chip-cancellation_pending'
                : 'zf-chip-warning'}"
            >
              {$t(absenceRequestTypeLabelKey(absence))}
            </span>
          </div>
          <div class="tab-num text-hint">
            {absenceKindLabel(absence.kind)} · {fmtDateShort(
              absence.start_date,
            )} -
            {fmtDateShort(absence.end_date)}
          </div>
          {#if absence.comment}
            <div class="absence-comment" title={absence.comment}>
              {absence.comment}
            </div>
          {/if}
        </div>
        <div class="zf-actions">
          <button
            class="zf-btn-icon-sm zf-btn-approve"
            on:click={() => onApproveAbsence(absence)}
          >
            <Icon name="Check" size={14} />
          </button>
          <button
            class="zf-btn-icon-sm zf-btn-reject"
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
        <div class="mt-8">{$t("No pending requests")}</div>
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
    font-size: 0.75rem;
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
    font-size: 0.875rem;
  }
</style>
