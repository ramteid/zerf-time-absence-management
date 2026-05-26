<script>
  import { t } from "../i18n.js";
  import { fmtDateTime, fmtWeekLabel } from "../format.js";
  import Icon from "../Icons.svelte";
  import Dialog from "../Dialog.svelte";
  import { userNameFromRows } from "../lib/domain/users.js";

  export let item;
  export let users;
  export let onClose;
  export let onApprove;
  export let onReject;
</script>

<Dialog title={$t("Edit Request Details")} onClose={onClose}>
  <div style="display:flex;flex-direction:column;gap:10px">
    <div>
      <div class="zf-label">{$t("Employee")}</div>
      <div style="font-weight:500">{userNameFromRows(item.user_id, users)}</div>
    </div>
    <div>
      <div class="zf-label">{$t("Type")}</div>
      <div><span class="zf-chip zf-chip-pending">{$t("Edit request")}</span></div>
    </div>
    <div>
      <div class="zf-label">{$t("Week")}</div>
      <div class="tab-num">
        {fmtWeekLabel(item.week_start)}
      </div>
    </div>
    <div>
      <div class="zf-label">{$t("Requested at")}</div>
      <div class="tab-num" style="font-size:12px">{fmtDateTime(item.created_at)}</div>
    </div>
    {#if item.reason}
      <div>
        <div class="zf-label">{$t("Reason")}</div>
        <div style="font-size:13px;white-space:pre-wrap;word-break:break-word">{item.reason}</div>
      </div>
    {/if}
  </div>
  <svelte:fragment slot="footer">
    <button class="zf-btn" on:click={onClose}>{$t("Close")}</button>
    <span style="flex:1"></span>
    <button class="zf-btn zf-btn-danger" on:click={() => onReject(item.id)}>
      <Icon name="X" size={14} />{$t("Reject")}
    </button>
    <button class="zf-btn zf-btn-primary" on:click={() => onApprove(item.id)}>
      <Icon name="Check" size={14} />{$t("Approve")}
    </button>
  </svelte:fragment>
</Dialog>
