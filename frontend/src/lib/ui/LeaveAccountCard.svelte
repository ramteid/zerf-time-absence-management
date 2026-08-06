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
  <div class="leave-account-card-title">
    <span
      class="leave-account-card-dot"
      style:background={balance.color || "#64748b"}
    ></span>
    <span>{$t(balance.category_name)}</span>
  </div>

  <div class="stat-card-label">{$t("Available")}</div>
  <div
    class="stat-card-value tab-num"
    style:color={available < 0 ? "var(--danger-text)" : "var(--success-text)"}
  >
    {formatDayCount(available)}
  </div>
  <div class="stat-card-sub">
    {$t("Entitlement")}
    {formatDayCount(balance.annual_entitlement)} · {$t("Taken")}
    {formatDayCount(balance.already_taken)} · {$t("Approved planned")}
    {formatDayCount(balance.approved_upcoming)} · {$t("Requested")}
    {formatDayCount(balance.requested)}
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
    min-width: 0;
    padding: 14px 16px;
  }

  .leave-account-card-title {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    min-width: 0;
    font-size: 0.9375rem;
    font-weight: 400;
    line-height: 1.3;
    min-height: calc(1.3em * 2);
    margin-bottom: 10px;
  }

  .leave-account-card-dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    flex: 0 0 auto;
    margin-top: 5px;
  }

  .leave-account-card-carryover {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
    border-top: 1px solid var(--border);
    margin-top: 12px;
    padding-top: 10px;
    color: var(--warning-text);
  }

  .leave-account-card-carryover > div {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .leave-account-card-carryover strong {
    font-weight: 400;
  }

  .leave-account-card-carryover.expired {
    color: var(--danger-text);
  }

  .leave-account-card-carryover-total {
    color: var(--text-tertiary);
    font-size: 0.8125rem;
    font-weight: 400;
  }

  .leave-account-card-carryover-status {
    color: var(--text-tertiary);
    font-size: 0.8125rem;
  }

  @media (max-width: 560px) {
    .leave-account-card-carryover {
      align-items: flex-start;
      flex-direction: column;
    }
  }
</style>
