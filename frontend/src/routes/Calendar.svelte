<script>
  import { tick } from "svelte";
  import { api } from "../api.js";
  import { path, go, currentUser } from "../stores.js";
  import { t, absenceKindLabel } from "../i18n.js";
  import {
    fmtMonthYear,
    weekdayLabels,
    monday,
    addDays,
    isoDate,
    fmtDate,
    durMin,
    minToHM,
  } from "../format.js";
  import Icon from "../Icons.svelte";

  let entries = [];
  let holidays = [];
  let timeEntries = [];
  let year, month;
  let dlg;
  let popupCell = null;

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
    const first = new Date(year, month - 1, 1);
    const last = new Date(year, month, 0);
    try {
      timeEntries = await api(
        `/time-entries?from=${isoDate(first)}&to=${isoDate(last)}`,
      );
    } catch {
      timeEntries = [];
    }
  }
  $: year && month && load().catch(() => {});

  $: hMap = new Map(holidays.map((f) => [f.holiday_date, f.name]));

  // Strict own-user filter for absences. Time entries are already scoped
  // server-side to the current user, but we double-check defensively.
  $: myAbsences = entries.filter((e) => e.user_id === $currentUser?.id);
  $: myTimeEntries = timeEntries.filter(
    (e) => !$currentUser || e.user_id === $currentUser.id,
  );

  $: teMap = (() => {
    const map = new Map();
    for (const te of myTimeEntries) {
      const d =
        typeof te.entry_date === "string"
          ? te.entry_date.slice(0, 10)
          : isoDate(te.entry_date);
      if (!map.has(d)) map.set(d, []);
      map.get(d).push(te);
    }
    return map;
  })();

  // Distinct, accessible palette. Red is reserved for error states only.
  const HOLIDAY_COLOR = "#f59e0b";
  const WORK_COLOR = "#3b82f6";
  const absColorMap = {
    vacation: "#10b981",
    sick: "#8b5cf6",
    training: "#14b8a6",
    special_leave: "#ec4899",
    unpaid: "#64748b",
    general_absence: "#475569",
    absent: "#9ca3af",
  };

  function absColor(kind) {
    return absColorMap[kind] || "#9ca3af";
  }

  // Stable legend ordering matching visual priority.
  const LEGEND_ORDER = [
    "holiday",
    "vacation",
    "sick",
    "training",
    "special_leave",
    "unpaid",
    "work",
  ];

  // One color per day. Priority: public holiday > absence > work time.
  function dayEvent(c) {
    if (c.hol) {
      return { type: "holiday", color: HOLIDAY_COLOR, label: $t("Holiday") };
    }
    if (c.absences.length > 0) {
      const a = c.absences[0];
      return {
        type: a.kind,
        color: absColor(a.kind),
        label: absenceKindLabel(a.kind),
      };
    }
    if ((teMap.get(c.ds) || []).length > 0) {
      return { type: "work", color: WORK_COLOR, label: $t("Work time") };
    }
    return null;
  }

  // Build all events for a cell (for the day-detail modal).
  function cellEvents(c) {
    const evts = [];
    if (c.hol) {
      evts.push({ color: HOLIDAY_COLOR, label: $t("Holiday"), detail: c.hol });
    }
    for (const a of c.absences) {
      evts.push({
        color: absColor(a.kind),
        label: absenceKindLabel(a.kind),
        detail: a.comment || "",
      });
    }
    for (const e of teMap.get(c.ds) || []) {
      const start = e.start_time?.slice(0, 5) || "";
      const end = e.end_time?.slice(0, 5) || "";
      const dur = start && end ? minToHM(durMin(start, end)) : "";
      const range = start && end ? `${start}–${end}` : "";
      const detail = dur ? `${range} (${dur})` : range;
      evts.push({ color: WORK_COLOR, label: $t("Work time"), detail });
    }
    return evts;
  }

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
        absences: myAbsences.filter(
          (e) => ds >= e.start_date && ds <= e.end_date,
        ),
      });
      if (i >= 34 && other && (i + 1) % 7 === 0) break;
    }
    return out;
  })();

  // One legend entry per event type currently visible in the month,
  // sorted in stable priority order matching LEGEND_ORDER.
  $: legendItems = (() => {
    const seen = new Map();
    for (const c of cells) {
      if (c.other) continue;
      if (c.hol && !seen.has("holiday")) {
        seen.set("holiday", { color: HOLIDAY_COLOR, label: $t("Holiday") });
      }
      for (const a of c.absences) {
        if (!seen.has(a.kind)) {
          seen.set(a.kind, {
            color: absColor(a.kind),
            label: absenceKindLabel(a.kind),
          });
        }
      }
      if ((teMap.get(c.ds) || []).length > 0 && !seen.has("work")) {
        seen.set("work", { color: WORK_COLOR, label: $t("Work time") });
      }
    }
    return LEGEND_ORDER.filter((k) => seen.has(k)).map((k) => seen.get(k));
  })();

  async function clickDay(c) {
    const evts = cellEvents(c);
    if (evts.length === 0) return;
    popupCell = { ...c, events: evts };
    await tick();
    dlg?.showModal();
  }

  function closeDlg() {
    dlg?.close();
    popupCell = null;
  }
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
    <div class="cal-grid" style="margin-bottom:8px">
      {#each weekdayLabels() as wd}
        <div class="cal-head">{wd}</div>
      {/each}
    </div>
    <div class="cal-grid">
      {#each cells as c}
        {@const ev = dayEvent(c)}
        <div
          class="cal-day"
          class:today={c.today}
          class:weekend={c.weekend && !c.today}
          class:other-month={c.other}
          style={ev
            ? `border-left:3px solid ${ev.color};cursor:pointer`
            : "cursor:default"}
          on:click={() => clickDay(c)}
          on:keydown={(e) => {
            if (e.key === "Enter" || e.key === " ") clickDay(c);
          }}
          role="button"
          tabindex="0"
        >
          <div class="cal-day-number tab-num">{c.d.getDate()}</div>
          {#if ev}
            <div class="cal-event" style="background:{ev.color}">
              {ev.label}
            </div>
          {/if}
        </div>
      {/each}
    </div>
  </div>

  <div style="display:flex;gap:12px;margin-top:16px;flex-wrap:wrap">
    {#each legendItems as item}
      <div style="display:flex;align-items:center;gap:6px;font-size:12px">
        <span
          style="display:inline-block;width:12px;height:12px;border-radius:2px;background:{item.color}"
        ></span>
        <span>{item.label}</span>
      </div>
    {/each}
  </div>
</div>

<dialog bind:this={dlg} on:close={() => (popupCell = null)}>
  {#if popupCell}
    <header>
      <span style="flex:1">{fmtDate(popupCell.ds)}</span>
      <button class="kz-btn-icon-sm kz-btn-ghost" on:click={closeDlg}>
        <Icon name="X" size={16} />
      </button>
    </header>
    <div class="dialog-body">
      {#each popupCell.events as ev}
        <div
          style="display:flex;align-items:center;gap:8px;padding:6px 0;font-size:13px"
        >
          <span
            style="display:inline-block;width:10px;height:10px;border-radius:2px;background:{ev.color};flex-shrink:0"
          ></span>
          <span style="font-weight:500">{ev.label}</span>
          {#if ev.detail}
            <span style="color:var(--text-muted)">{ev.detail}</span>
          {/if}
        </div>
      {/each}
    </div>
    <footer>
      <span style="flex:1"></span>
      <button class="kz-btn" on:click={closeDlg}>{$t("Close")}</button>
    </footer>
  {/if}
</dialog>
