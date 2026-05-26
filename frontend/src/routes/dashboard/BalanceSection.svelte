<script>
  import { t, formatHours } from "../../i18n.js";
  import Icon from "../../Icons.svelte";
  import StatCard from "../../lib/ui/StatCard.svelte";

  export let isAssistantCurrentUser = false;
  export let overtimeLoading = false;
  export let submittedOvertimeBalanceMin = 0;
  export let overtimeBalanceMin = 0;
  export let currentMonthDiffMin = 0;
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
  <div class="dashboard-group-label" style="display:flex;align-items:center;gap:6px">
    {$t("My Balance")}
    <button
      class="zf-btn-icon-sm zf-btn-ghost"
      title={$t("help_my_balance")}
      on:click={() => onHelpToggle("balance")}
      style="color:var(--text-tertiary);font-size:14px;cursor:help"
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
      <StatCard
        label={$t("Overtime overview")}
        loading={overtimeLoading}
        color={submittedOvertimeBalanceMin < 0
          ? "var(--danger-text)"
          : "var(--success-text)"}
      >
        {formatHours((submittedOvertimeBalanceMin || 0) / 60)}
        <span slot="sub">
          {#if submittedOvertimeBalanceMin !== overtimeBalanceMin}
            {$t("Approved: {value}", { value: formatHours((overtimeBalanceMin || 0) / 60) })}
          {:else}
            {$t("This month: {value}", { value: formatHours((currentMonthDiffMin || 0) / 60) })}
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
      <span slot="sub" style="color:var(--text-tertiary);font-size:11px;margin-top:4px">
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
  .dashboard-help {
    font-size: 12px;
    color: var(--text-tertiary);
    margin-bottom: 12px;
    padding: 8px;
    background: var(--bg-muted);
    border-radius: var(--radius-sm);
  }

  .dashboard-card-error {
    font-size: 11px;
    margin-top: 4px;
  }
</style>
