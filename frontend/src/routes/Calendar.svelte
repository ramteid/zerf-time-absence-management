<script>
  import { api } from "../api.js";
  import { path, go, currentUser, categories, settings, earliestStartDate } from "../stores.js";
  import { t } from "../i18n.js";
  import {
    fmtMonthYear,
    weekdayLabels,
    monday,
    addDays,
    isoDate,
    appTodayDate,
    appTodayIsoDate,
    fmtDate,
  } from "../format.js";
  import Icon from "../Icons.svelte";
  import Dialog from "../Dialog.svelte";
  import {
    buildColorMap,
    calendarEventTitle,
    cellEvents,
  } from "../lib/domain/calendar.js";
  import { tracksOwnTime } from "../rolePolicy.js";

  let entries = [];
  let holidays = [];
  let timeEntries = [];
  let users = [];
  let year, month;
  // eslint-disable-next-line no-useless-assignment
  let popupCell = null;
  let loadSeq = 0;

  async function fallbackToEmpty(promise) {
    try {
      return await promise;
    } catch {
      return [];
    }
  }

  function calendarGridDateRange(loadYear, loadMonth) {
    const first = new Date(loadYear, loadMonth - 1, 1);
    const start = monday(first);
    let end = start;
    for (let dayOffset = 0; dayOffset < 42; dayOffset++) {
      const date = addDays(start, dayOffset);
      const other = date.getMonth() !== loadMonth - 1;
      end = date;
      if (dayOffset >= 34 && other && (dayOffset + 1) % 7 === 0) break;
    }
    return { start, end };
  }

  function yearsInRange(start, end) {
    const years = [];
    for (let y = start.getFullYear(); y <= end.getFullYear(); y++) {
      years.push(y);
    }
    return years;
  }

  $: {
    const queryString = $path.includes("?") ? $path.split("?")[1] : "";
    const searchParams = new URLSearchParams(queryString);
    const today = appTodayDate($settings?.timezone);
    year = Number(searchParams.get("year")) || today.getFullYear();
    month = Number(searchParams.get("month")) || today.getMonth() + 1;
    // Close any open day-detail popup when navigating to a different month.
    popupCell = null;
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
    const gridRange = calendarGridDateRange(loadYear, loadMonth);
    const holidayYears = yearsInRange(gridRange.start, gridRange.end);
    const isLead = $currentUser?.permissions?.can_approve ?? false;
    // Admins see all users via /time-entries/all (own entries included server-side).
    // Non-admin leads: /time-entries/all returns only direct reports (own entries are
    // excluded server-side), so fetch own entries separately and merge both lists.
    const isAdmin = $currentUser?.role === "admin";
    const isNonAdminLead = isLead && !isAdmin;
    try {
      const [nextEntries, nextHolidays, teamEntries, selfEntries, nextCategories, nextUsers] =
        await Promise.all([
          fallbackToEmpty(api(`/absences/calendar?month=${monthString}`)),
          Promise.all(
            holidayYears.map((holidayYear) =>
              fallbackToEmpty(api(`/holidays?year=${holidayYear}`)),
            ),
          ).then((yearRows) => yearRows.flat()),
          isLead
            ? fallbackToEmpty(api(`/time-entries/all?from=${from}&to=${to}`))
            : fallbackToEmpty(api(`/time-entries?from=${from}&to=${to}`)),
          isNonAdminLead
            ? fallbackToEmpty(api(`/time-entries?from=${from}&to=${to}`))
            : Promise.resolve([]),
          api("/categories").catch(() => $categories),
          isLead ? fallbackToEmpty(api("/users")) : Promise.resolve([]),
        ]);
      if (seq !== loadSeq) return;
      entries = nextEntries;
      holidays = nextHolidays;
      timeEntries = [...teamEntries, ...selfEntries];
      categories.set(nextCategories);
      // Pure-admin users (tracks_time=false) never have calendar entries; drop
      // them from the lookup so they can't appear in calendar event labels.
      // Inactive users are also excluded.
      users = (nextUsers || []).filter((u) => tracksOwnTime(u) && u.active !== false);
    } catch {
      if (seq !== loadSeq) return;
      entries = [];
      holidays = [];
      timeEntries = [];
      users = [];
    }
  }
  $: loadKey =
    year && month
      ? [
          year,
          month,
          $currentUser?.id ?? "",
          $currentUser?.role ?? "",
          $currentUser?.permissions?.can_approve ? "lead" : "self",
          $settings?.timezone ?? "",
        ].join(":")
      : "";
  $: loadKey && load().catch(() => {});

  $: holidayByDate = new Map(
    holidays.map((holiday) => [holiday.holiday_date, holiday.name]),
  );

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

  // ── Heading: "Team Calendar" for team leads and admins (they can always see
  // other users' data), "My Calendar" for employees and assistants.
  $: calendarHeadingKey =
    $currentUser?.role === "team_lead" || $currentUser?.role === "admin"
      ? "Team Calendar"
      : "My Calendar";

  // ── Earliest navigable month: derived from the global earliest start date
  // (the month the first user started). The prev button is disabled when the
  // current month is already at or before this lower bound.
  $: earliestMonth = $earliestStartDate?.slice(0, 7) ?? null; // "YYYY-MM" or null
  $: currentMonthStr = `${year}-${String(month).padStart(2, "0")}`;
  // Leads and admins are exempt: their own start_date may be NULL (excluded
  // from the SQL MIN), so the global earliest may be newer than their own data.
  $: isLeadOrAdmin = $currentUser?.role === "team_lead" || $currentUser?.role === "admin";
  $: prevDisabled = !isLeadOrAdmin && earliestMonth != null && currentMonthStr <= earliestMonth;

  // ── Weekend column visibility: only render Sat/Sun columns when at least
  // one visible cell on Saturday or Sunday actually has events. If either
  // weekend day has events, both columns are shown so the week stays paired.
  $: showWeekends = eventCells.some(
    (cell) => cell.weekend && cell.events.length > 0,
  );
  $: visibleWeekdayLabels = showWeekends
    ? weekdayLabels()
    : weekdayLabels().slice(0, 5);
  $: visibleEventCells = showWeekends
    ? eventCells
    : eventCells.filter((cell) => !cell.weekend);
  $: calGridColumns = showWeekends ? 7 : 5;

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

  function clickDay(cell) {
    const cellEventsList = cell.events;
    if (cellEventsList.length === 0) return;
    popupCell = { ...cell, events: cellEventsList };
  }

  function monthFromPath() {
    const queryString = $path.includes("?") ? $path.split("?")[1] : "";
    const searchParams = new URLSearchParams(queryString);
    const today = appTodayDate($settings?.timezone);
    return {
      year: Number(searchParams.get("year")) || year || today.getFullYear(),
      month: Number(searchParams.get("month")) || month || today.getMonth() + 1,
    };
  }

  function navigateMonth(delta) {
    const current = monthFromPath();
    const target = new Date(current.year, current.month - 1 + delta, 1);
    go(`/calendar?year=${target.getFullYear()}&month=${target.getMonth() + 1}`);
  }
</script>

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t(calendarHeadingKey)}</h1>
  </div>
  <div class="top-bar-actions calendar-top-actions">
    <div class="zf-nav-slider">
      <button
        type="button"
        class="zf-btn zf-btn-ghost"
        aria-label={$t("Previous month")}
        on:click={() => navigateMonth(-1)}
        disabled={prevDisabled}
      >
        <Icon name="ChevLeft" size={16} />
      </button>
      <span class="nav-label tab-num" style="min-width:70px">
        {fmtMonthYear(new Date(year, month - 1, 1))}
      </span>
      <button
        type="button"
        class="zf-btn zf-btn-ghost"
        aria-label={$t("Next month")}
        on:click={() => navigateMonth(1)}
      >
        <Icon name="ChevRight" size={16} />
      </button>
    </div>
  </div>
</div>

<div class="content-area">
  <div class="zf-card" style="padding:16px">
    <div
      class="cal-grid"
      style="grid-template-columns:repeat({calGridColumns},minmax(28px,1fr));margin-bottom:8px"
    >
      {#each visibleWeekdayLabels as wd (wd)}
        <div class="cal-head">{wd}</div>
      {/each}
    </div>
    <div
      class="cal-grid"
      style="grid-template-columns:repeat({calGridColumns},minmax(28px,1fr))"
    >
      {#each visibleEventCells as c (c.ds)}
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
              {#each evts.slice(0, 3) as ev (ev.key)}
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
    {#each legendItems as item (item.label)}
      <div style="display:flex;align-items:center;gap:6px;font-size:12px">
        <span
          style="display:inline-block;width:12px;height:12px;border-radius:2px;background:{item.color}"
        ></span>
        <span>{item.label}</span>
      </div>
    {/each}
  </div>
</div>

{#if popupCell}
  <Dialog title={fmtDate(popupCell.ds)} onClose={() => (popupCell = null)}>
    {#each popupCell.events as ev (ev.key)}
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
    <svelte:fragment slot="footer">
      <span style="flex:1"></span>
      <button class="zf-btn" on:click={() => (popupCell = null)}>{$t("Close")}</button>
    </svelte:fragment>
  </Dialog>
{/if}
