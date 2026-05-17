<script>
  import { tick } from "svelte";
  import { api } from "../api.js";
  import { path, go, currentUser, categories, settings } from "../stores.js";
  import { t, absenceKindLabel } from "../i18n.js";
  import {
    fmtMonthYear,
    weekdayLabels,
    monday,
    addDays,
    isoDate,
    appTodayDate,
    appTodayIsoDate,
    fmtDate,
    durMin,
    minToHM,
  } from "../format.js";
  import Icon from "../Icons.svelte";
  import { HOLIDAY_COLOR, ABSENCE_COLORS, FALLBACK_COLORS } from "../colors.js";

  let entries = [];
  let holidays = [];
  let timeEntries = [];
  let users = [];
  let year, month;
  let dlg;
  let popupCell = null;
  let loadSeq = 0;

  $: {
    const queryString = $path.includes("?") ? $path.split("?")[1] : "";
    const searchParams = new URLSearchParams(queryString);
    const today = appTodayDate($settings?.timezone);
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
    const from = isoDate(firstDayOfMonth);
    const to = isoDate(lastDayOfMonth);
    const isLead = $currentUser?.permissions?.can_approve ?? false;
    try {
      const [nextEntries, nextHolidays, nextTimeEntries, nextCategories, nextUsers] =
        await Promise.all([
          api(`/absences/calendar?month=${monthString}`),
          api(`/holidays?year=${loadYear}`),
          api(
            isLead
              ? `/time-entries/all?from=${from}&to=${to}`
              : `/time-entries?from=${from}&to=${to}`,
          ).catch(() => []),
          api("/categories").catch(() => $categories),
          isLead ? api("/users").catch(() => []) : Promise.resolve([]),
        ]);
      if (seq !== loadSeq) return;
      entries = nextEntries;
      holidays = nextHolidays;
      timeEntries = nextTimeEntries;
      categories.set(nextCategories);
      users = nextUsers;
    } catch {
      if (seq !== loadSeq) return;
      entries = [];
      holidays = [];
      timeEntries = [];
      users = [];
    }
  }
  $: year && month && load().catch(() => {});

  $: holidayByDate = new Map(
    holidays.map((holiday) => [holiday.holiday_date, holiday.name]),
  );

  // For leads, the API already scopes to direct reports; for others, to self.
  // Rejected entries are excluded from the calendar view in all cases.
  $: calTimeEntries = timeEntries.filter((e) => e.status !== "rejected");

  $: userById = new Map(users.map((u) => [u.id, u]));

  $: teMap = (() => {
    const timeEntriesByDate = new Map();
    for (const timeEntry of calTimeEntries) {
      const entryDateKey =
        typeof timeEntry.entry_date === "string"
          ? timeEntry.entry_date.slice(0, 10)
          : isoDate(timeEntry.entry_date);
      if (!timeEntriesByDate.has(entryDateKey))
        timeEntriesByDate.set(entryDateKey, []);
      timeEntriesByDate.get(entryDateKey).push(timeEntry);
    }
    return timeEntriesByDate;
  })();

  $: categoryById = new Map(
    $categories.map((category) => [category.id, category]),
  );

  function absColor(kind) {
    return ABSENCE_COLORS[kind] || ABSENCE_COLORS.absent;
  }

  function normalizeColor(color) {
    return /^#[0-9a-f]{6}$/i.test(color || "") ? color.toLowerCase() : null;
  }

  function fallbackColor(offset = 0, used = new Set()) {
    for (
      let colorIndex = 0;
      colorIndex < FALLBACK_COLORS.length;
      colorIndex++
    ) {
      const color =
        FALLBACK_COLORS[(offset + colorIndex) % FALLBACK_COLORS.length];
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

  function rawCellEvents(cell, entryMap, categoryMap, translate, userMap = new Map(), currentUserId = null) {
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
      const durationLabel =
        startTime && endTime ? minToHM(durMin(startTime, endTime)) : "";
      const timeRange = startTime && endTime ? `${startTime} - ${endTime}` : "";
      const timeDetail = durationLabel ? `${timeRange} (${durationLabel})` : timeRange;
      const isOwn = entry.user_id === currentUserId;
      const entryUser = !isOwn ? userMap.get(entry.user_id) : null;
      const userName = entryUser
        ? `${entryUser.first_name} ${entryUser.last_name}`
        : null;
      const detail = userName ? `${userName} – ${timeDetail}` : timeDetail;
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
    // Reserved colors for holidays and absences. Work-category events must not
    // use these, but holiday/absence events always keep their designated color.
    const reservedColors = new Set([
      HOLIDAY_COLOR.toLowerCase(),
      ...Object.values(ABSENCE_COLORS).map((color) => color.toLowerCase()),
    ]);
    const assigned = new Map();
    const used = new Set();
    for (const cell of baseCells) {
      if (cell.other) continue;
      for (const event of rawCellEvents(
        cell,
        entryMap,
        categoryMap,
        translate,
      )) {
        if (assigned.has(event.key)) continue;
        const isWorkEvent = event.key.startsWith("work:");
        const blocked = new Set([...used, ...reservedColors]);
        let color = normalizeColor(event.color) || fallbackColor(assigned.size, blocked);
        if (isWorkEvent) {
          // Work events must avoid both already-used and reserved colors.
          if (used.has(color) || reservedColors.has(color)) {
            color = fallbackColor(assigned.size, blocked);
          }
        } else {
          // Holiday/absence events keep their color unless another event already took it.
          if (used.has(color)) color = fallbackColor(assigned.size, blocked);
        }
        assigned.set(event.key, color);
        used.add(color);
      }
    }
    return assigned;
  }

  function cellEvents(cell, entryMap, categoryMap, colorMap, translate, userMap, currentUserId) {
    return rawCellEvents(cell, entryMap, categoryMap, translate, userMap, currentUserId).map(
      (event) => ({
        ...event,
        color: colorMap.get(event.key) || event.color,
      }),
    );
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

  $: todayStr = appTodayIsoDate($settings?.timezone);

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
        absences: entries.filter(
          (entry) =>
            dateString >= entry.start_date && dateString <= entry.end_date,
        ),
      });
      if (dayOffset >= 34 && other && (dayOffset + 1) % 7 === 0) break;
    }
    return nextCells;
  })();

  $: colorByKey = buildColorMap(cells, teMap, categoryById, $t);
  $: eventCells = cells.map((cell) => ({
    ...cell,
    events: cellEvents(cell, teMap, categoryById, colorByKey, $t, userById, $currentUser?.id),
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
    <div class="zf-nav-slider">
      <button
        class="zf-btn zf-btn-ghost"
        on:click={() => go("/calendar" + prev)}
      >
        <Icon name="ChevLeft" size={16} />
      </button>
      <span class="nav-label tab-num" style="min-width:70px">
        {fmtMonthYear(new Date(year, month - 1, 1))}
      </span>
      <button
        class="zf-btn zf-btn-ghost"
        on:click={() => go("/calendar" + next)}
      >
        <Icon name="ChevRight" size={16} />
      </button>
    </div>
  </div>
</div>

<div class="content-area">
  <div class="zf-card" style="padding:16px">
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
      <button class="zf-btn-icon-sm zf-btn-ghost" on:click={closeDlg}>
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
      <button class="zf-btn" on:click={closeDlg}>{$t("Close")}</button>
    </footer>
  {/if}
</dialog>
