<script>
  import { fly } from "svelte/transition";
  import { currentUser, settings } from "../../stores.js";
  import { t, absenceKindLabel } from "../../i18n.js";
  import {
    addDays,
    appTodayDate,
    fmtDateShort,
    isoDate,
    monday,
    parseDate,
  } from "../../format.js";
  import Icon from "../../Icons.svelte";
  import { getTeamAbsences } from "../../lib/api/dashboardApi.js";

  export let users = [];

  let today = appTodayDate();
  $: today = appTodayDate($settings?.timezone);

  let week = isoDate(monday(appTodayDate()));
  let data = [];
  let direction = 1;
  let isLeadView = false;

  async function loadWeek(weekStartDate) {
    isLeadView = $currentUser?.permissions?.can_approve || false;
    if (!isLeadView) return;
    try {
      const weekEnd = isoDate(addDays(parseDate(weekStartDate), 6));
      const params = new URLSearchParams({
        from: weekStartDate,
        to: weekEnd,
        status: "approved",
      });
      data = await getTeamAbsences(params);
    } catch {
      data = [];
    }
  }

  function prevWeek() {
    direction = -1;
    week = isoDate(addDays(parseDate(week), -7));
    loadWeek(week);
  }

  function nextWeek() {
    direction = 1;
    week = isoDate(addDays(parseDate(week), 7));
    loadWeek(week);
  }

  function toToday() {
    direction = 0;
    week = isoDate(monday(today));
    loadWeek(week);
  }

  loadWeek(week);

  $: sortedData = [...data].sort((a, b) => {
    const ua = users.find((u) => u.id === a.user_id);
    const ub = users.find((u) => u.id === b.user_id);
    return (ua?.last_name || "").localeCompare(ub?.last_name || "") ||
      (ua?.first_name || "").localeCompare(ub?.first_name || "");
  });
</script>

<div class="zf-card" style="margin-top:16px;overflow:hidden">
  <div class="card-header">
    <Icon name="Users" size={15} sw={1.5} />
    <span class="card-header-title">{$t("Who is absent")}</span>
    <div class="absence-date-controls">
      <div class="absence-week-picker">
        <button
          class="zf-btn zf-btn-icon-sm zf-btn-ghost"
          on:click={prevWeek}
          aria-label={$t("Previous week")}
        >
          <Icon name="ChevLeft" size={16} />
        </button>
        <button
          class="zf-btn zf-btn-ghost absence-week-range"
          on:click={toToday}
          title={$t("Today")}
        >
          {fmtDateShort(week)} -
          {fmtDateShort(isoDate(addDays(parseDate(week), 6)))}
        </button>
        <button
          class="zf-btn zf-btn-icon-sm zf-btn-ghost"
          on:click={nextWeek}
          aria-label={$t("Next week")}
        >
          <Icon name="ChevRight" size={16} />
        </button>
      </div>
    </div>
  </div>

  {#key week}
    <div class="dropdown-slider" in:fly={{ x: direction * 80, duration: 200 }}>
      {#if data.length === 0}
        <div style="padding:12px;color:var(--text-tertiary);font-size:13px">
          {$t("No absences this week.")}
        </div>
      {:else}
        {#each sortedData as absence (absence.user_id)}
          {@const absentUser = users.find((u) => u.id === absence.user_id)}
          <div class="dropdown-slider-item">
            <div>
              <div style="font-weight:500;font-size:13px">
                {absentUser
                  ? `${absentUser.first_name} ${absentUser.last_name}`
                  : `#${absence.user_id}`}
              </div>
              <div style="font-size:12px;color:var(--text-tertiary)">
                {absenceKindLabel(absence.kind)} · {fmtDateShort(absence.start_date)}{#if absence.start_date !== absence.end_date} - {fmtDateShort(absence.end_date)}{/if}
              </div>
            </div>
          </div>
        {/each}
      {/if}
    </div>
  {/key}
</div>

<style>
  .absence-date-controls {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
  }

  .absence-week-picker {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .absence-week-range {
    color: var(--text-tertiary);
    font-size: 12px;
    min-width: 108px;
    justify-content: center;
    padding: 2px 6px;
    height: auto;
  }

  .absence-week-range:hover {
    color: var(--text-primary);
  }
</style>
