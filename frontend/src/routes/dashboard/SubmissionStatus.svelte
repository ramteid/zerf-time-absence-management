<script>
  import { t } from "../../i18n.js";
  import Icon from "../../Icons.svelte";
  import StatusDonut from "../../lib/ui/StatusDonut.svelte";
  import { appTodayDate, fmtMonthName } from "../../format.js";

  // Payload of GET /reports/submission-status, or null while it loads.
  export let status = null;
  export let activeHelp = null;
  export let onHelpToggle = () => {};
  export let onOpen = () => {};
  // Switch between the previous month and the running one — see Dashboard.svelte.
  export let onShowCurrentMonth = () => {};
  export let onShowPreviousMonth = () => {};

  const today = appTodayDate();
  const currentMonthName = fmtMonthName(today);
  const previousMonthName = fmtMonthName(
    new Date(today.getFullYear(), today.getMonth() - 1, 1),
  );
  // The whole "YYYY-MM" is compared: matching on the month number alone would
  // also match the same month of another year.
  const currentPeriod = `${today.getFullYear()}-${String(
    today.getMonth() + 1,
  ).padStart(2, "0")}`;
  $: showingCurrentMonth = status?.period === currentPeriod;
</script>

<div class="zf-card submissions-card">
  <div class="card-header">
    <Icon name="FileText" size={15} sw={1.5} />
    <span class="card-header-title">{$t("Submissions")}</span>
    <button
      class="zf-btn-icon-sm zf-btn-ghost zf-help-icon"
      title={$t("help_submissions_card")}
      on:click={() => onHelpToggle("submissions")}
    >
      <Icon name="Info" size={14} />
    </button>
  </div>
  {#if activeHelp === "submissions"}
    <div class="dashboard-help submissions-help">
      {$t("help_submissions_card")}
    </div>
  {/if}

  <button
    class="submissions-body submissions-card-button"
    on:click={onOpen}
    disabled={!status}
  >
    <StatusDonut
      ready={status?.ready ?? 0}
      awaitingApproval={status?.awaiting_approval ?? 0}
      notSubmitted={status?.not_submitted ?? 0}
    />
    <div>
      <div class="submissions-headline">
        {$t("{ready} of {total} done")
          .replace("{ready}", status?.ready ?? 0)
          .replace("{total}", status?.total ?? 0)}
      </div>
      <div class="submissions-sub">
        {status?.period_label ?? ""}
        {#if showingCurrentMonth}
          · {$t("still running")}
        {/if}
      </div>
    </div>
  </button>
  {#if status}
    <button
      class="zf-btn zf-btn-ghost zf-btn-sm submissions-peek-btn"
      on:click={showingCurrentMonth ? onShowPreviousMonth : onShowCurrentMonth}
    >
      {$t("Show {month}").replace(
        "{month}",
        showingCurrentMonth ? previousMonthName : currentMonthName,
      )}
    </button>
  {/if}
</div>

<style>
  .submissions-card {
    width: 100%;
    text-align: left;
    min-width: 0;
  }

  /* The content is a button so the card remains keyboard reachable while the
     header keeps the same structure as the other dashboard cards. */
  .submissions-card-button {
    font: inherit;
    color: inherit;
    cursor: pointer;
    border: 0;
    background: transparent;
    width: 100%;
    text-align: left;
  }

  .submissions-card-button:hover:not(:disabled) {
    background: var(--bg-subtle);
  }

  .submissions-card-button:disabled {
    cursor: default;
  }

  .submissions-body {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 16px;
    min-width: 0;
  }

  .submissions-help {
    font-size: 0.8125rem;
    color: var(--text-tertiary);
    margin: 12px 16px 0;
    padding: 8px;
    background: var(--bg-muted);
    border-radius: var(--radius-sm);
  }

  .submissions-headline {
    font-size: 1.125rem;
    font-weight: 600;
  }

  .submissions-sub {
    font-size: 0.8125rem;
    color: var(--text-tertiary);
  }

  .submissions-peek-btn {
    margin: 0 16px 12px;
    padding-left: 0;
    padding-right: 0;
    height: auto;
  }
</style>
