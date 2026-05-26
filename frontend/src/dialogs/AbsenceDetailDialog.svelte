<script>
  import { t, absenceKindLabel, statusLabel, formatDayCount } from "../i18n.js";
  import { fmtDate, fmtDateTime } from "../format.js";
  import Icon from "../Icons.svelte";
  import Dialog from "../Dialog.svelte";

  export let absence;
  export let onClose;
  export let onEdit = null;
  export let onCancel = null;
  export let cancelLabel = "";
</script>

<Dialog title={absenceKindLabel(absence.kind)} onClose={onClose}>
  <div style="display:flex;flex-direction:column;gap:10px">
    <div class="field-row">
      <div>
        <div class="zf-label">{$t("From")}</div>
        <div class="tab-num">{fmtDate(absence.start_date)}</div>
      </div>
      <div>
        <div class="zf-label">{$t("To")}</div>
        <div class="tab-num">{fmtDate(absence.end_date)}</div>
      </div>
      <div>
        <div class="zf-label">{$t("Days")}</div>
        <div class="tab-num">{absence.days == null ? "-" : formatDayCount(absence.days)}</div>
      </div>
    </div>
    <div>
      <div class="zf-label">{$t("Status")}</div>
      <span class="zf-chip zf-chip-{absence.status}">{statusLabel(absence.status)}</span>
    </div>
    {#if absence.comment}
      <div>
        <div class="zf-label">{$t("Comment")}</div>
        <div style="white-space:pre-wrap;font-size:13px">{absence.comment}</div>
      </div>
    {/if}
    {#if absence.rejection_reason}
      <div>
        <div class="zf-label">{$t("Rejection reason")}</div>
        <div style="white-space:pre-wrap;font-size:13px;color:var(--danger-text)">
          {absence.rejection_reason}
        </div>
      </div>
    {/if}
    <div>
      <div class="zf-label">{$t("Requested at")}</div>
      <div class="tab-num" style="font-size:12px">{fmtDateTime(absence.created_at)}</div>
    </div>
  </div>
  <svelte:fragment slot="footer">
    <button class="zf-btn" on:click={onClose}>{$t("Close")}</button>
    <span style="flex:1"></span>
    {#if absence.cancellable && onCancel}
      <button class="zf-btn zf-btn-danger" on:click={() => onCancel(absence)}>
        {cancelLabel}
      </button>
    {/if}
    {#if absence.editable && onEdit}
      <button class="zf-btn zf-btn-primary" on:click={() => onEdit(absence)}>
        <Icon name="Edit" size={13} />{$t("Edit")}
      </button>
    {/if}
  </svelte:fragment>
</Dialog>
