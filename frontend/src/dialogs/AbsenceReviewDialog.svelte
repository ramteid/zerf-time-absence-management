<script>
  import { t, absenceKindLabel } from "../i18n.js";
  import { fmtDate, fmtDateTime } from "../format.js";
  import Icon from "../Icons.svelte";
  import Dialog from "../Dialog.svelte";
  import {
    absenceDiffRows,
    absenceRequestTypeLabelKey,
  } from "../lib/domain/dashboard.js";
  import { userNameFromRows } from "../lib/domain/users.js";

  export let absence;
  export let users;
  export let onClose;
  export let onApprove;
  export let onReject;

  $: requestTypeLabel = $t(absenceRequestTypeLabelKey(absence));
  $: diffRows = absence.review_type === "change" ? absenceDiffRows(absence, $t) : [];
</script>

<Dialog title={$t("Absence Request Details")} onClose={onClose}>
  <div style="display:flex;flex-direction:column;gap:10px">
    <div>
      <div class="zf-label">{$t("Employee")}</div>
      <div style="font-weight:500">{userNameFromRows(absence.user_id, users)}</div>
    </div>
    <div>
      <div class="zf-label">{$t("Absence Type")}</div>
      <div>{absenceKindLabel(absence.kind)}</div>
    </div>
    <div>
      <div class="zf-label">{$t("Request Type")}</div>
      <div>
        <span
          class="zf-chip {absence.status === 'cancellation_pending' ? 'zf-chip-cancellation_pending' : 'zf-chip-warning'}"
        >
          {requestTypeLabel}
        </span>
      </div>
    </div>
    <div class="field-row">
      <div>
        <div class="zf-label">{$t("From")}</div>
        <div class="tab-num">{fmtDate(absence.start_date)}</div>
      </div>
      <div>
        <div class="zf-label">{$t("To")}</div>
        <div class="tab-num">{fmtDate(absence.end_date)}</div>
      </div>
    </div>
    {#if absence.comment}
      <div>
        <div class="zf-label">{$t("Comment")}</div>
        <div style="white-space:pre-wrap;font-size:13px">{absence.comment}</div>
      </div>
    {/if}
    <div>
      <div class="zf-label">{$t("Requested at")}</div>
      <div class="tab-num" style="font-size:12px">
        {fmtDateTime(absence.created_at)}
      </div>
    </div>
    {#if absence.review_type === "change"}
      {#if diffRows.length}
        <div>
          <div class="zf-label">{$t("Changes")}</div>
          <div class="change-diff-list">
            {#each diffRows as row (row.field)}
              <div class="change-diff-row">
                <div class="change-diff-field">{row.field}</div>
                <div class="change-diff-before">{row.before}</div>
                <div class="change-diff-arrow">→</div>
                <div class="change-diff-after">{row.after}</div>
              </div>
            {/each}
          </div>
        </div>
      {:else}
        <div style="font-size:12px;color:var(--text-tertiary)">
          {$t("Diff unavailable for this request.")}
        </div>
      {/if}
    {/if}
  </div>
  <svelte:fragment slot="footer">
    <button class="zf-btn" on:click={onClose}>{$t("Close")}</button>
    <span style="flex:1"></span>
    <button class="zf-btn zf-btn-danger" on:click={() => onReject(absence)}>
      <Icon name="X" size={14} />{$t("Reject")}
    </button>
    <button class="zf-btn zf-btn-primary" on:click={() => onApprove(absence)}>
      <Icon name="Check" size={14} />{$t("Approve")}
    </button>
  </svelte:fragment>
</Dialog>

<style>
  .change-diff-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .change-diff-row {
    display: grid;
    grid-template-columns: minmax(70px, auto) 1fr auto 1fr;
    gap: 8px;
    align-items: center;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--bg-subtle);
    padding: 8px 10px;
    font-size: 12px;
  }

  .change-diff-field {
    color: var(--text-secondary);
    font-weight: 500;
  }

  .change-diff-before {
    color: var(--text-tertiary);
    text-decoration: line-through;
  }

  .change-diff-arrow {
    color: var(--text-tertiary);
  }

  .change-diff-after {
    color: var(--text-primary);
    font-weight: 500;
  }
</style>
