<script>
  import { t, formatHours } from "../i18n.js";
  import { fmtWeekLabel } from "../format.js";
  import Icon from "../Icons.svelte";
  import Dialog from "../Dialog.svelte";
  import { userNameFromRows } from "../lib/domain/users.js";

  export let week;
  export let users;
  export let busy = false;
  export let onClose;
  export let onApprove;
  export let onReject;
</script>

<Dialog title={$t("Week Approvals")} onClose={onClose}>
  <svelte:fragment slot="title">
    <span style="flex:1">
      {$t("Week Approvals")} · {userNameFromRows(week.user_id, users)}
    </span>
  </svelte:fragment>
  <div class="tab-num" style="font-size:12px;color:var(--text-secondary)">
    {fmtWeekLabel(week.week_start)}
  </div>

  <div style="display:flex;gap:8px;flex-wrap:wrap">
    <span class="zf-chip zf-chip-approved">{formatHours(week.total_min / 60)}</span>
  </div>
  <svelte:fragment slot="footer">
    <button class="zf-btn" on:click={onClose} disabled={busy}>
      {$t("Close")}
    </button>
    <span style="flex:1"></span>
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
