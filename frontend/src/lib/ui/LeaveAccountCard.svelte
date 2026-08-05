<script>
  import { t, formatDayCount } from "../../i18n.js";
  import { fmtDate } from "../../format.js";

  export let balance;
  export let year;

  $: available = balance?.available ?? 0;
  $: carryoverDays = balance?.carryover_days ?? 0;
  $: carryoverRemaining = balance?.carryover_remaining ?? 0;
</script>

<article
  class="zf-card leave-account-card"
  data-testid={`leave-account-card-${balance.category_id}`}
>
  <header class="leave-account-card-header">
    <div class="leave-account-card-name">
      <span
        class="leave-account-card-dot"
        style:background={balance.color || "#64748b"}
      ></span>
      <span>{$t(balance.category_name)}</span>
    </div>
    <div class:negative={available < 0} class="leave-account-card-available">
      <span class="leave-account-card-label">{$t("Available")}</span>
      <strong class="tab-num">{formatDayCount(available)}</strong>
    </div>
  </header>

  <div class="leave-account-card-stats">
    <div>
      <span>{$t("Entitlement")}</span>
      <strong class="tab-num"
        >{formatDayCount(balance.annual_entitlement)}</strong
      >
    </div>
    <div>
      <span>{$t("Taken")}</span>
      <strong class="tab-num">{formatDayCount(balance.already_taken)}</strong>
    </div>
    <div>
      <span>{$t("Approved planned")}</span>
      <strong class="tab-num"
        >{formatDayCount(balance.approved_upcoming)}</strong
      >
    </div>
    <div>
      <span>{$t("Requested")}</span>
      <strong class="tab-num">{formatDayCount(balance.requested)}</strong>
    </div>
  </div>

  {#if carryoverDays > 0}
    <div
      class:expired={balance.carryover_expired}
      class="leave-account-card-carryover"
    >
      <div>
        <span>{$t("Carryover from {year}", { year: year - 1 })}</span>
        <strong class="tab-num"
          >{formatDayCount(balance.carryover_expired ? 0 : carryoverRemaining)}
          <span class="leave-account-card-carryover-total"
            >/ {formatDayCount(carryoverDays)}</span
          ></strong
        >
      </div>
      {#if balance.carryover_expiry}
        <span class="leave-account-card-carryover-status">
          {balance.carryover_expired
            ? $t("Expired on {date}", {
                date: fmtDate(balance.carryover_expiry),
              })
            : $t("Expires on {date}", {
                date: fmtDate(balance.carryover_expiry),
              })}
        </span>
      {/if}
    </div>
  {/if}
</article>

<style>
  .leave-account-card {
    display: flex;
    flex-direction: column;
    gap: 14px;
    min-width: 0;
    padding: 16px 20px;
  }

  .leave-account-card-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
  }

  .leave-account-card-name {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    font-weight: 600;
  }

  .leave-account-card-dot {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    flex: 0 0 auto;
  }

  .leave-account-card-available {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    color: var(--success-text);
  }

  .leave-account-card-available.negative {
    color: var(--danger-text);
  }

  .leave-account-card-available strong {
    font-size: 1.375rem;
    line-height: 1.1;
  }

  .leave-account-card-label,
  .leave-account-card-stats span,
  .leave-account-card-carryover-status {
    color: var(--text-tertiary);
    font-size: 0.8125rem;
  }

  .leave-account-card-stats {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 8px;
  }

  .leave-account-card-stats > div {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .leave-account-card-stats strong,
  .leave-account-card-carryover strong {
    font-weight: 600;
  }

  .leave-account-card-carryover {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
    border-top: 1px solid var(--border);
    padding-top: 10px;
    color: var(--warning-text);
  }

  .leave-account-card-carryover > div {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .leave-account-card-carryover.expired {
    color: var(--danger-text);
  }

  .leave-account-card-carryover-total {
    color: var(--text-tertiary);
    font-size: 0.8125rem;
    font-weight: 400;
  }

  @media (max-width: 560px) {
    .leave-account-card-stats {
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: 10px 16px;
    }

    .leave-account-card-carryover {
      align-items: flex-start;
      flex-direction: column;
    }
  }
</style>
