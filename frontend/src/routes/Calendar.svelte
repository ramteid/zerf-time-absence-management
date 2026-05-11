<script>
  import { tick } from "svelte";
  import { api } from "../api.js";
  import { path, go, currentUser, categories } from "../stores.js";
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
  let loadSeq = 0;

  $: {
    const queryString = $path.includes("?") ? $path.split("?")[1] : "";
    const searchParams = new URLSearchParams(queryString);
    const today = new Date();
    year = Number(searchParams.get("year")) || today.getFullYear();
    month = Number(searchParams.get("month")) || today.getMonth() + 1;
  }

  async function load() {
    const seq = ++loadSeq;
    const loadYear = year;
    const loadMonth = month;
    const monthString = `${loadYear}-${String(loadMonth).padStart(2, "0")}`;
    const firstDayOfMonth = new Date(loadYear, loadMonth - 1, 1);
    const lastDayOfMonth = new Date(loadYear, loadMonth, 0);
    const [nextEntries, nextHolidays, nextTimeEntries, nextCategories] =
      await Promise.all([
        api(`/absences/calendar?month=${monthString}`),
        api(`/holidays?year=${loadYear}`),
        api(`/time-entries?from=${isoDate(firstDayOfMonth)}&to=${isoDate(lastDayOfMonth)}`).catch(
          () => [],
        ),
        api("/categories").catch(() => $categories),
      ]);
    if (seq !== loadSeq) return;
    entries = nextEntries;
    holidays = nextHolidays;
    timeEntries = nextTimeEntries;
    categories.set(nextCategories);
  }
  $: year && month && load().catch(() => {});

  $: holidayByDate = new Map(holidays.map((holiday) => [holiday.holiday_date, holiday.name]));

  // The calendar API is already team-scoped. Time entries remain personal,
  // so we still filter those defensively.
  $: myTimeEntries = timeEntries.filter((e) => e.user_id === $currentUser?.id);

  $: teMap = (() => {
    const timeEntriesByDate = new Map();
    for (const timeEntry of myTimeEntries) {
      const entryDateKey =
        typeof timeEntry.entry_date === "string"
          ? timeEntry.entry_date.slice(0, 10)
          : isoDate(timeEntry.entry_date);
      if (!timeEntriesByDate.has(entryDateKey)) timeEntriesByDate.set(entryDateKey, []);
      timeEntriesByDate.get(entryDateKey).push(timeEntry);
    }
    return timeEntriesByDate;
  })();

  $: categoryById = new Map(
    $categories.map((category) => [category.id, category]),
  );

  // Distinct, accessible palette. Red is reserved for error states only.
  // Amber (#f59e0b) is reserved for holidays; yellow-adjacent tones are avoided in fallbacks.
  const HOLIDAY_COLOR = "#f59e0b";
  const FALLBACK_COLORS = [
    "#2563eb",
    "#10b981",
    "#8b5cf6",
    "#14b8a6",
    "#ec4899",
    "#64748b",
    "#0f766e",
    "#7c3aed",
    "#0891b2",
    "#d946ef",
    "#4f46e5",
    "#0d9488",
  ];
  const absColorMap = {
    vacation: "#f59e0b",
    sick: "#ef4444",
    training: "#3b82f6",
    special_leave: "#a855f7",
    unpaid: "#64748b",
    general_absence: "#6b7280",
    absent: "#9ca3af",
  };

  function absColor(kind) {
    return absColorMap[kind] || "#9ca3af";
  }

  function normalizeColor(color) {
    return /^#[0-9a-f]{6}$/i.test(color || "") ? color.toLowerCase() : null;
  }

  function fallbackColor(offset = 0, used = new Set()) {
    for (let colorIndex = 0; colorIndex < FALLBACK_COLORS.length; colorIndex++) {
      const color = FALLBACK_COLORS[(offset + colorIndex) % FALLBACK_COLORS.length];
      if (!used.has(color.toLowerCase())) return color;
    }
    const hue = (offset * 47) % 360;
    return `hsl(${hue} 70% 38%)`;
  }

  function categoryForEntry(entry, categoryMap) {
    return categoryMap.get(entry.category_id) || null;
  }

  function workLabel(entry, categoryMap) {
    return categoryForEntry(entry, categoryMap)?.name || "Work time";
  }

  function workBaseColor(entry, offset, categoryMap) {
    return (
      normalizeColor(categoryForEntry(entry, categoryMap)?.color) ||
      fallbackColor(offset)
    );
  }

  function absenceDetail(absence) {
    return [absence.name, absence.comment].filter(Boolean).join(" - ");
  }

  function rawCellEvents(cell, entryMap, categoryMap, translate) {
    const events = [];
    if (cell.hol) {
      events.push({
        key: "holiday",
        color: HOLIDAY_COLOR,
        label: translate("Holiday"),
        detail: cell.hol,
      });
    }
    for (const absence of cell.absences) {
      const label = absenceKindLabel(absence.kind);
      events.push({
        key: `absence:${absence.kind}`,
        color: absColor(absence.kind),
        label,
        title: label,
        detail: absenceDetail(absence),
      });
    }
    for (const entry of entryMap.get(cell.ds) || []) {
      const startTime = entry.start_time?.slice(0, 5) || "";
      const endTime = entry.end_time?.slice(0, 5) || "";
      const durationLabel = startTime && endTime ? minToHM(durMin(startTime, endTime)) : "";
      const timeRange = startTime && endTime ? `${startTime} - ${endTime}` : "";
      const detail = durationLabel ? `${timeRange} (${durationLabel})` : timeRange;
      events.push({
        key: `work:${entry.category_id ?? "unknown"}`,
        color: workBaseColor(entry, events.length, categoryMap),
        label: translate(workLabel(entry, categoryMap)),
        detail,
      });
    }
    return events;
  }

  function buildColorMap(baseCells, entryMap, categoryMap, translate) {
    // Pre-seed reserved colors so work categories can never accidentally clash
    // with holiday or absence colors, even in months where none of those appear.
    const used = new Set([
      HOLIDAY_COLOR.toLowerCase(),
      ...Object.values(absColorMap).map((color) => color.toLowerCase()),
    ]);
    const assigned = new Map();
    for (const cell of baseCells) {
      if (cell.other) continue;
      for (const event of rawCellEvents(cell, entryMap, categoryMap, translate)) {
        if (assigned.has(event.key)) continue;
        let color =
          normalizeColor(event.color) || fallbackColor(assigned.size, used);
        if (used.has(color)) color = fallbackColor(assigned.size, used);
        assigned.set(event.key, color);
        used.add(color);
      }
    }
    return assigned;
  }

  function cellEvents(cell, entryMap, categoryMap, colorMap, translate) {
    return rawCellEvents(cell, entryMap, categoryMap, translate).map((event) => ({
      ...event,
      color: colorMap.get(event.key) || event.color,
    }));
  }

  function calendarEventTitle(event) {
    return String(event?.title || event?.detail || event?.label || "").trim();
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
    const nextCells = [];
    for (let dayOffset = 0; dayOffset < 42; dayOffset++) {
      const date = addDays(start, dayOffset);
      const dateString = isoDate(date);
      const other = date.getMonth() !== month - 1;
      const weekdayIndex = (date.getDay() + 6) % 7;
      nextCells.push({
        d: date,
        ds: dateString,
        other,
        // Calendar weekend styling is date-based (Saturday/Sunday), not user-contract-based.
        weekend: weekdayIndex >= 5,
        today: dateString === todayStr,
        hol: holidayByDate.get(dateString),
        absences: entries.filter((entry) => dateString >= entry.start_date && dateString <= entry.end_date),
      });
      if (dayOffset >= 34 && other && (dayOffset + 1) % 7 === 0) break;
    }
    return nextCells;
  })();

  $: colorByKey = buildColorMap(cells, teMap, categoryById, $t);
  $: eventCells = cells.map((cell) => ({
    ...cell,
    events: cellEvents(cell, teMap, categoryById, colorByKey, $t),
  }));

  $: legendItems = (() => {
    const seen = new Map();
    for (const cell of eventCells) {
      if (cell.other) continue;
      for (const event of cell.events) {
        if (!seen.has(event.key)) {
          seen.set(event.key, { color: event.color, label: event.label });
        }
      }
    }
    return [...seen.values()];
  })();

  async function clickDay(cell) {
    const cellEventsList = cell.events;
    if (cellEventsList.length === 0) return;
    popupCell = { ...cell, events: cellEventsList };
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
  </div>
  <div class="top-bar-actions calendar-top-actions">
    <div class="kz-nav-slider">
      <button
        class="kz-btn kz-btn-ghost"
        on:click={() => go("/calendar" + prev)}
      >
        <Icon name="ChevLeft" size={16} />
      </button>
      <span class="nav-label tab-num" style="min-width:70px">
        {fmtMonthYear(new Date(year, month - 1, 1))}
      </span>
      <button
        class="kz-btn kz-btn-ghost"
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
      {#each eventCells as c}
        {@const evts = c.events}
        <button
          type="button"
          class="cal-day"
          class:has-events={evts.length > 0}
          class:today={c.today}
          class:weekend={c.weekend && !c.today}
          class:other-month={c.other}
          style={evts.length
            ? `border-left:3px solid ${evts[0].color};cursor:pointer`
            : "cursor:default"}
          on:click={() => clickDay(c)}
          disabled={evts.length === 0}
        >
          <div class="cal-day-number tab-num">{c.d.getDate()}</div>
          {#if evts.length}
            <div class="cal-events">
              {#each evts.slice(0, 3) as ev}
                <div class="cal-event" style="background:{ev.color}">
                  {calendarEventTitle(ev)}
                </div>
              {/each}
              {#if evts.length > 3}
                <div class="cal-more">+{evts.length - 3}</div>
              {/if}
            </div>
          {/if}
        </button>
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
          <span style="font-weight:500">{ev.popupLabel || ev.label}</span>
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
