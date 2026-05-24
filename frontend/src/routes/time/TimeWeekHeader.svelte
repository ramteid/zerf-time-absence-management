<script>
  import { createEventDispatcher } from "svelte";
  import Icon from "../../Icons.svelte";
  import { t } from "../../i18n.js";
  import { fmtDateShort, fmtWeekLabel } from "../../format.js";

  export let weekFrom = null;
  export let weekTo = null;
  export let isAssistant = false;
  export let contractHours = "";
  export let drafts = [];
  export let canRequestReopen = false;
  export let isAtOrPastCurrentWeek = false;

  const dispatch = createEventDispatcher();
</script>

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t("Time Entry")}</h1>
    {#if weekFrom}
      <div class="top-bar-subtitle">
        {fmtWeekLabel(weekFrom)}
        {#if !isAssistant}
          · {contractHours} {$t("contract")}
        {/if}
      </div>
    {/if}
  </div>
  <div class="top-bar-actions time-top-bar-actions">
    {#if weekFrom}
      <div class="zf-nav-slider time-week-picker">
        <button class="zf-btn zf-btn-ghost" on:click={() => dispatch("prev")}>
          <Icon name="ChevLeft" size={16} />
        </button>
        <span class="nav-label tab-num time-week-label">
          {fmtDateShort(weekFrom)} &ndash; {fmtDateShort(weekTo)}
        </span>
        <button
          class="zf-btn zf-btn-ghost"
          on:click={() => dispatch("next")}
          disabled={isAtOrPastCurrentWeek}
        >
          <Icon name="ChevRight" size={16} />
        </button>
      </div>
    {/if}

    <div class="time-submit-stack">
      {#if drafts.length || !canRequestReopen}
        <button
          class="zf-btn zf-btn-primary time-submit-button"
          on:click={() => dispatch("submit")}
          disabled={!drafts.length}
        >
          <Icon name="Send" size={14} />{$t("Submit Week")}
        </button>
      {/if}

      {#if canRequestReopen}
        {#if drafts.length}
          <button
            class="zf-btn zf-btn-sm"
            on:click={() => dispatch("requestReopen")}
            title={$t("Request edit")}
          >
            <Icon name="Edit" size={13} />{$t("Request edit")}
          </button>
        {:else}
          <button
            class="zf-btn time-submit-button time-reopen-button"
            on:click={() => dispatch("requestReopen")}
            title={$t("Request edit")}
          >
            <Icon name="Edit" size={14} />{$t("Request edit")}
          </button>
        {/if}
      {/if}
    </div>
  </div>
</div>
