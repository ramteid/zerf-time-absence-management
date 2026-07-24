<script>
  import { t, formatHours } from "../i18n.js";
  import { fmtWeekLabel } from "../format.js";
  import Icon from "../Icons.svelte";
  import Dialog from "../Dialog.svelte";
  import { userNameFromRows } from "../lib/domain/users.js";
  import { go } from "../stores.js";

  export let week;
  export let users;
  export let busy = false;
  export let onClose;
  export let onApprove;
  export let onReject;

  // Deep-link into the per-person detailed report for this exact user/week so
  // approvers can spot-check entries before approving without manually
  // navigating Reports and picking the user and date range.
  function goToReport() {
    go(
      `/reports?user=${week.user_id}&from=${week.week_start}&to=${week.week_end}`,
    );
    onClose();
  }
</script>

<Dialog title={$t("Week Approvals")} {onClose}>
  <svelte:fragment slot="title">
    <span class="flex-1">
      {$t("Week Approvals")} · {userNameFromRows(week.user_id, users)}
    </span>
  </svelte:fragment>
  <div class="tab-num fs-13 text-secondary">
    {fmtWeekLabel(week.week_start)}
  </div>

  <div class="zf-btn-row">
    <span class="zf-chip zf-chip-approved"
      >{formatHours(week.total_min / 60)}</span
    >
  </div>
  <svelte:fragment slot="footer">
    <button class="zf-btn" on:click={onClose} disabled={busy}>
      {$t("Close")}
    </button>
    <span class="flex-1"></span>
    <button class="zf-btn" on:click={goToReport} disabled={busy}>
      <Icon name="BarChart" size={14} />{$t("View in report")}
    </button>
    <button
      class="zf-btn zf-btn-danger"
      on:click={() => onReject(week)}
      disabled={busy}
    >
      <Icon name="X" size={14} />{$t("Reject")}
    </button>
    <button
      class="zf-btn zf-btn-primary"
      on:click={() => onApprove(week)}
      disabled={busy}
    >
      <Icon name="Check" size={14} />{$t("Approve")}
    </button>
  </svelte:fragment>
</Dialog>

<style></style>
