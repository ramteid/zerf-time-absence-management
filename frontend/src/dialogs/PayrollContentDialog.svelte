<script>
  import { t } from "../i18n.js";
  import Dialog from "../Dialog.svelte";
  import { fmtDate, minToHM } from "../format.js";

  // Payload of GET /reports/payroll-content.
  export let content;
  export let onClose;

  function signedMinutes(minutes) {
    const formatted = minToHM(minutes ?? 0);
    return (minutes ?? 0) > 0 ? `+${formatted}` : formatted;
  }

  function daysLabel(days) {
    return $t("{days} days").replace("{days}", days);
  }

  function categoryAndPeriod(row) {
    return `${row.category} · ${fmtDate(row.from)} – ${fmtDate(row.to)}`;
  }

  // One entry per block of the report, in the order the report prints them:
  // absences first, then the hours tables, then the catch-up days. `detail`
  // and `amount` turn a row into its two right-hand cells (`amount` may be
  // absent when the report states no figure, e.g. cancelled absences).
  const SECTIONS = [
    {
      kind: "absence",
      title: "Absences",
      detail: categoryAndPeriod,
      amount: (row) => daysLabel(row.days),
    },
    {
      kind: "hours",
      title: "Working days and hours",
      detail: (row) => daysLabel(row.days),
      amount: (row) => minToHM(row.minutes ?? 0),
    },
    {
      kind: "late_absence",
      title: "Reported later",
      hint: "Absences recorded after the report for their own month had already been sent. They go into this month's report with the days they actually cover.",
      detail: categoryAndPeriod,
      amount: (row) => daysLabel(row.days),
    },
    {
      kind: "absence_cancellation",
      title: "Cancelled absences",
      hint: "Review earlier payroll reports and correct any affected declarations.",
      detail: categoryAndPeriod,
    },
    {
      kind: "absence_review",
      title: "Absences requiring review",
      hint: "Check earlier reports to determine the absence days for these periods.",
      detail: categoryAndPeriod,
    },
    {
      kind: "late_hours",
      title: "Corrections to earlier months",
      hint: "Working time that changed after the report for its month was sent. Positive hours add time and negative hours reduce it. This report lists each correction under the day it belongs to.",
      detail: (row) => fmtDate(row.from),
      amount: (row) => signedMinutes(row.minutes),
    },
  ];

  // What the scheduled send is still waiting for goes first: it is the thing
  // a reader opening the list most likely wants to resolve.
  $: missing = content?.readiness?.missing ?? [];

  $: sections = SECTIONS.map((section) => ({
    ...section,
    rows: (content?.rows ?? []).filter((row) => row.kind === section.kind),
  })).filter((section) => section.rows.length > 0);
</script>

<Dialog title={$t("Payroll Report")} {onClose} huge>
  <svelte:fragment slot="title">
    <span class="flex-1">
      {$t("Payroll Report")} · {content.period_label}
    </span>
  </svelte:fragment>

  <p class="payroll-intro">
    {#if content.in_progress}
      {$t("What this month is shaping up to report.")}
    {:else if content.sent}
      {$t("What this month's report contained.")}
    {:else}
      {$t("What this month's report will contain.")}
    {/if}
  </p>

  {#if sections.length === 0 && missing.length === 0}
    <div class="payroll-empty">{$t("Nothing to report for this month.")}</div>
  {:else}
    <div class="payroll-sections">
      {#if missing.length > 0}
        <section class="payroll-section">
          <h3 class="payroll-heading">{$t("Still missing")}</h3>
          <p class="payroll-hint">
            {$t(
              "The report is sent automatically once all of these are settled.",
            )}
          </p>
          <ul class="payroll-rows">
            {#each missing as person, index (`missing-${index}`)}
              <li class="payroll-row no-amount">
                <span class="payroll-name">{person.name}</span>
                <span class="payroll-detail">{person.reason}</span>
              </li>
            {/each}
          </ul>
        </section>
      {/if}
      {#each sections as section (section.kind)}
        <section class="payroll-section">
          <h3 class="payroll-heading">{$t(section.title)}</h3>
          {#if section.hint}
            <p class="payroll-hint">{$t(section.hint)}</p>
          {/if}
          <ul class="payroll-rows">
            {#each section.rows as row, index (`${section.kind}-${index}`)}
              <li class="payroll-row" class:no-amount={!section.amount}>
                <span class="payroll-name">{row.name}</span>
                <span class="payroll-detail">{section.detail(row)}</span>
                {#if section.amount}
                  <span class="payroll-amount">{section.amount(row)}</span>
                {/if}
              </li>
            {/each}
          </ul>
        </section>
      {/each}
    </div>
  {/if}

  <svelte:fragment slot="footer">
    <button class="zf-btn" on:click={onClose}>{$t("Close")}</button>
  </svelte:fragment>
</Dialog>

<style>
  /* One vertical rhythm: 28px between blocks, 4px between a heading and its
     hint, 12px before a list. */
  .payroll-intro {
    margin: 0;
    font-size: 0.875rem;
    color: var(--text-secondary);
  }

  /* Wins over the dialog body's generic 14px gap; margins do not collapse in
     its flex column, so the intro carries no bottom margin of its own. */
  .payroll-sections {
    margin-top: 20px;
    display: flex;
    flex-direction: column;
    gap: 28px;
  }

  .payroll-heading {
    margin: 0;
    font-size: 0.8125rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--text-secondary);
  }

  .payroll-hint {
    margin: 4px 0 0;
    max-width: 70ch;
    font-size: 0.8125rem;
    line-height: 1.45;
    color: var(--text-tertiary);
  }

  /* The lists are never height-capped: the dialog body scrolls as a whole. */
  .payroll-rows {
    list-style: none;
    margin: 12px 0 0;
    padding: 0;
    border-top: 1px solid var(--border);
  }

  /* Three real columns shared by every row of a list: person, details,
     figure. Long names wrap instead of being cut off. */
  .payroll-row {
    display: grid;
    grid-template-columns: minmax(0, 1.2fr) minmax(0, 1.6fr) minmax(7rem, auto);
    column-gap: 24px;
    align-items: baseline;
    padding: 10px 8px;
    border-bottom: 1px solid var(--border);
  }

  .payroll-row.no-amount {
    grid-template-columns: minmax(0, 1.2fr) minmax(0, 2.6fr);
  }

  .payroll-name {
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .payroll-detail {
    min-width: 0;
    overflow-wrap: anywhere;
    color: var(--text-tertiary);
    font-size: 0.875rem;
  }

  .payroll-amount {
    text-align: right;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }

  .payroll-empty {
    padding: 32px 24px;
    text-align: center;
    color: var(--text-tertiary);
    font-size: 0.875rem;
  }

  /* Phone: name and figure share the first line, the details wrap below. */
  @media (max-width: 640px) {
    .payroll-row,
    .payroll-row.no-amount {
      grid-template-columns: minmax(0, 1fr) auto;
      grid-template-areas:
        "name amount"
        "detail detail";
      row-gap: 2px;
      padding: 10px 2px;
    }

    .payroll-name {
      grid-area: name;
    }

    .payroll-detail {
      grid-area: detail;
    }

    .payroll-amount {
      grid-area: amount;
    }
  }
</style>
