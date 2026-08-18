<script>
  import { t, formatHours } from "../../i18n.js";
  import { fmtDate } from "../../format.js";
  import Icon from "../../Icons.svelte";
  import StatCard from "../../lib/ui/StatCard.svelte";

  export let isAssistantCurrentUser = false;
  export let overtimeLoading = false;
  export let overtimeBalanceMin = 0;
  export let currentMonthDiffMin = 0;
  /// End of the last fully approved week — the date the balance is stated as of.
  export let balanceAsOf = null;
  export let overtimeError = "";
  export let monthSubmissionLoading = false;
  export let allWeeksApproved = false;
  export let allWeeksSubmitted = false;
  export let currentWeekOpen = false;
  export let monthSubmissionError = "";
  export let activeHelp = null;
  export let onHelpToggle = () => {};
</script>

<div class="dashboard-group">
  <div class="dashboard-group-label zf-row">
    {$t("My Balance")}
    <button
      class="zf-btn-icon-sm zf-btn-ghost zf-help-icon"
      title={$t("help_my_balance")}
      on:click={() => onHelpToggle("balance")}
    >
      <Icon name="Info" size={14} />
    </button>
  </div>
  {#if activeHelp === "balance"}
    <div class="dashboard-help">
      {$t("help_my_balance")}
    </div>
  {/if}

  <div class="stat-cards">
    {#if !isAssistantCurrentUser}
      <!-- Approved hours only: the balance runs to the end of the last fully
           approved week, so the "as of" line below states which date it means. -->
      <StatCard
        label={$t("Overtime overview")}
        loading={overtimeLoading}
        color={overtimeBalanceMin < 0
          ? "var(--danger-text)"
          : "var(--success-text)"}
      >
        {formatHours((overtimeBalanceMin || 0) / 60)}
        <span slot="sub">
          {$t("This month: {value}", {
            value: formatHours((currentMonthDiffMin || 0) / 60),
          })}
          {#if balanceAsOf}
            <span class="balance-as-of"
              >{$t("As of {date}", { date: fmtDate(balanceAsOf) })}</span
            >
          {/if}
        </span>
      </StatCard>
      {#if overtimeError}
        <div class="error-text dashboard-card-error">
          {$t("Overtime data unavailable.")}
        </div>
      {/if}
    {/if}

    <StatCard
      label={$t("Submissions")}
      loading={monthSubmissionLoading}
      color={allWeeksApproved ? "var(--success-text)" : "var(--warning-text)"}
    >
      {#if allWeeksApproved}
        {$t("All submitted and approved")}
      {:else if allWeeksSubmitted}
        {$t("All submitted (approvals pending)")}
      {:else}
        {$t("Weeks missing")}
      {/if}
      <span slot="sub" class="stat-note">
        {#if currentWeekOpen}
          {$t("Current week: still open")}
        {/if}
      </span>
    </StatCard>
    {#if monthSubmissionError}
      <div class="error-text dashboard-card-error">
        {$t("Could not check submission status.")}
      </div>
    {/if}
  </div>
</div>

<style>
  /* Second line inside the tile's sub slot: the date the balance refers to. */
  .balance-as-of {
    display: block;
    color: var(--text-tertiary);
  }

  .stat-note {
    color: var(--text-tertiary);
    font-size: 0.75rem;
    margin-top: 4px;
  }

  .dashboard-help {
    font-size: 0.8125rem;
    color: var(--text-tertiary);
    margin-bottom: 12px;
    padding: 8px;
    background: var(--bg-muted);
    border-radius: var(--radius-sm);
  }

  .dashboard-card-error {
    font-size: 0.75rem;
    margin-top: 4px;
  }
</style>
