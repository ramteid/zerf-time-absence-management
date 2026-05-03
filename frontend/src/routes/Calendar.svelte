<script>
  import { api } from "../api.js";
  import { path, go } from "../stores.js";
  import { t, absenceKindLabel } from "../i18n.js";
  import {
    fmtMonthYear,
    weekdayLabels,
    monday,
    addDays,
    isoDate,
  } from "../format.js";
  import Icon from "../Icons.svelte";

  let entries = [];
  let holidays = [];
  let year, month;

  $: {
    const q = $path.includes("?") ? $path.split("?")[1] : "";
    const p = new URLSearchParams(q);
    const today = new Date();
    year = Number(p.get("year")) || today.getFullYear();
    month = Number(p.get("month")) || today.getMonth() + 1;
  }

  async function load() {
    const ms = `${year}-${String(month).padStart(2, "0")}`;
    entries = await api(`/absences/calendar?month=${ms}`);
    holidays = await api(`/holidays?year=${year}`);
  }
  $: year && month && load().catch(() => {});
  $: hMap = new Map(holidays.map((f) => [f.holiday_date, f.name]));

  $: prev =
    month === 1
      ? `?year=${year - 1}&month=12`
      : `?year=${year}&month=${month - 1}`;
  $: next =
    month === 12
      ? `?year=${year + 1}&month=1`
      : `?year=${year}&month=${month + 1}`;

  const todayStr = isoDate(new Date());

  $: cells = (() => {
    const first = new Date(year, month - 1, 1);
    const start = monday(first);
    const out = [];
    for (let i = 0; i < 42; i++) {
      const d = addDays(start, i);
      const ds = isoDate(d);
      const other = d.getMonth() !== month - 1;
      const wd = (d.getDay() + 6) % 7;
      out.push({
        d,
        ds,
        other,
        weekend: wd >= 5,
        today: ds === todayStr,
        hol: hMap.get(ds),
        absences: entries.filter((e) => ds >= e.start_date && ds <= e.end_date),
      });
      if (i >= 34 && other && (i + 1) % 7 === 0) break;
    }
    return out;
  })();
</script>

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t("Calendar")}</h1>
    <div class="top-bar-subtitle">
      {fmtMonthYear(new Date(year, month - 1, 1))}
    </div>
  </div>
  <div class="top-bar-actions">
    <div style="display:flex;align-items:center;gap:4px">
      <button
        class="kz-btn kz-btn-icon-sm kz-btn-ghost"
        on:click={() => go("/calendar" + prev)}
      >
        <Icon name="ChevLeft" size={16} />
      </button>
      <span
        class="tab-num"
        style="font-size:13.5px;font-weight:500;min-width:140px;text-align:center"
      >
        {fmtMonthYear(new Date(year, month - 1, 1))}
      </span>
      <button
        class="kz-btn kz-btn-icon-sm kz-btn-ghost"
        on:click={() => go("/calendar" + next)}
      >
        <Icon name="ChevRight" size={16} />
      </button>
    </div>
  </div>
</div>

<div class="content-area">
  <div class="kz-card" style="padding:16px">
    <div class="cal-scroll-wrap">
      <div class="cal-grid" style="margin-bottom:8px">
        {#each weekdayLabels() as wd}
          <div class="cal-head">{wd}</div>
        {/each}
      </div>
      <div class="cal-grid">
        {#each cells as c}
          <div
            class="cal-day"
            class:today={c.today}
            class:weekend={c.weekend && !c.today}
            class:other-month={c.other}
            title={c.hol || ""}
          >
            <div class="cal-day-number tab-num">{c.d.getDate()}</div>
            {#if c.hol}
              <div class="cal-event" style="background:var(--danger)">
                {c.hol}
              </div>
            {/if}
            {#each c.absences as e}
              <div
                class="cal-event abs-{e.kind}"
                title="{e.name} · {absenceKindLabel(e.kind)}{e.comment
                  ? ' · ' + e.comment
                  : ''}"
              >
                {e.name}{e.half_day ? " ½" : ""}
              </div>
            {/each}
          </div>
        {/each}
      </div>
    </div>
    <!-- end cal-scroll-wrap -->
  </div>

  <div style="display:flex;gap:12px;margin-top:16px;flex-wrap:wrap">
    {#each ["vacation", "sick", "training", "special_leave", "unpaid", "general_absence"] as k}
      <div style="display:flex;align-items:center;gap:6px;font-size:12px">
        <span
          class="cal-event abs-{k}"
          style="padding:2px 6px;display:inline-block"
        >
          {absenceKindLabel(k)}
        </span>
      </div>
    {/each}
  </div>
</div>
