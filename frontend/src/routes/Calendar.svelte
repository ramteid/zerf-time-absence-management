<script>
  import { api } from "../api.js";
  import {
    path,
    go,
    currentUser,
    categories,
    settings,
    earliestStartDate,
    absenceCategories,
  } from "../stores.js";
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
    rawCellEvents,
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
  let activeFilters = new Set(); // colorKey -> active

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
      const [
        nextEntries,
        nextHolidays,
        teamEntries,
        selfEntries,
        nextCategories,
        nextUsers,
      ] = await Promise.all([
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
      users = (nextUsers || []).filter(
        (u) => tracksOwnTime(u) && u.active !== false,
      );
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

  $: absCatBySlug = new Map($absenceCategories.map((c) => [c.slug, c]));
  $: colorByKey = buildColorMap(cells, teMap, categoryById, absCatBySlug, $t);
  $: eventCells = cells.map((cell) => {
    const allEvents = cellEvents(
      cell,
      teMap,
      categoryById,
      colorByKey,
      absCatBySlug,
      $t,
      userById,
      $currentUser?.id,
    );
    // Filter events based on active filters
    const filteredEvents =
      activeFilters.size === 0
        ? allEvents
        : allEvents.filter((event) => activeFilters.has(event.colorKey));
    return { ...cell, events: filteredEvents };
  });

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
  $: isLeadOrAdmin =
    $currentUser?.role === "team_lead" || $currentUser?.role === "admin";
  $: prevDisabled =
    !isLeadOrAdmin && earliestMonth != null && currentMonthStr <= earliestMonth;

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

  $: allLegendItems = (() => {
    const seen = new Map();
    // Build legend from all cells (before filtering)
    for (const cell of cells) {
      if (cell.other) continue;
      // Use rawCellEvents to get unfiltered events for legend generation
      const rawEvents = rawCellEvents(
        cell,
        teMap,
        categoryById,
        absCatBySlug,
        $t,
        userById,
        $currentUser?.id,
      );
      for (const event of rawEvents) {
        if (!seen.has(event.colorKey)) {
          seen.set(event.colorKey, {
            colorKey: event.colorKey,
            color: colorByKey.get(event.colorKey) || event.color,
            label: event.label
          });
        }
      }
    }
    return [...seen.values()];
  })();

  $: {
    // Sync activeFilters when legend changes (e.g., month navigation).
    // On first load (activeFilters.size === 0), enable all categories.
    // On subsequent loads, remove filters for categories no longer in the legend.
    if (allLegendItems.length > 0) {
      const currentKeys = new Set(allLegendItems.map((item) => item.colorKey));
      if (activeFilters.size === 0) {
        activeFilters = new Set(currentKeys);
      } else {
        activeFilters = new Set([...activeFilters].filter((key) => currentKeys.has(key)));
      }
    }
  }

  function toggleFilter(colorKey) {
    const newFilters = new Set(activeFilters);
    if (newFilters.has(colorKey)) {
      newFilters.delete(colorKey);
    } else {
      newFilters.add(colorKey);
    }
    activeFilters = newFilters;
  }

  $: legendItems = (() => {
    // Sort legend items consistently: holidays first, then absences, then work categories
    const sortKey = (item) => {
      if (item.colorKey === "holiday") return [0, item.label];
      if (item.colorKey.startsWith("absence:")) return [1, item.label];
      return [2, item.label];
    };
    return allLegendItems
      .map((item) => ({
        ...item,
        active: activeFilters.has(item.colorKey),
      }))
      .sort((a, b) => {
        const [aGroup, aLabel] = sortKey(a);
        const [bGroup, bLabel] = sortKey(b);
        if (aGroup !== bGroup) return aGroup - bGroup;
        return aLabel.localeCompare(bLabel);
      });
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
      <span class="nav-label tab-num cal-month-label">
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
  <div class="zf-card cal-card">
    <div
      class="cal-grid mb-8"
      style:grid-template-columns={`repeat(${calGridColumns},minmax(28px,1fr))`}
    >
      {#each visibleWeekdayLabels as wd (wd)}
        <div class="cal-head">{wd}</div>
      {/each}
    </div>
    <div
      class="cal-grid"
      style:grid-template-columns={`repeat(${calGridColumns},minmax(28px,1fr))`}
    >
      {#each visibleEventCells as c (c.ds)}
        {@const evts = c.events}
        {#if c.weekend && evts.length === 0}
          <!-- Keeps the grid slot so the shared 7-column grid stays aligned,
               without showing a visible box for a weekend day with no
               entries. Saturday and Sunday are hidden independently: one
               can be shown while the other stays hidden in the same week. -->
          <div class="cal-day-spacer" aria-hidden="true"></div>
        {:else}
          <button
            type="button"
            class="cal-day"
            class:has-events={evts.length > 0}
            class:today={c.today}
            class:weekend={c.weekend && !c.today}
            class:other-month={c.other}
            style:border-left={evts.length ? `3px solid ${evts[0].color}` : null}
            on:click={() => clickDay(c)}
            disabled={evts.length === 0}
          >
            <div class="cal-day-number tab-num">{c.d.getDate()}</div>
            {#if evts.length}
              <div class="cal-events">
                {#each evts.slice(0, 3) as ev (ev.key)}
                  <div class="cal-event" style:background={ev.color}>
                    {calendarEventTitle(ev)}
                  </div>
                {/each}
                {#if evts.length > 3}
                  <div class="cal-more">+{evts.length - 3}</div>
                {/if}
              </div>
            {/if}
          </button>
        {/if}
      {/each}
    </div>
  </div>

  <div class="cal-legend">
    {#each legendItems as item (item.colorKey)}
      <button
        type="button"
        class="cal-legend-item"
        class:inactive={!item.active}
        on:click={() => toggleFilter(item.colorKey)}
        title={item.active ? $t("Hide") : $t("Show")}
        aria-label="{item.active ? $t('Hide') : $t('Show')}: {item.label}"
      >
        <span class="cal-swatch" style:background={item.color}></span>
        <span>{item.label}</span>
      </button>
    {/each}
  </div>
</div>

{#if popupCell}
  <Dialog title={fmtDate(popupCell.ds)} onClose={() => (popupCell = null)}>
    {#each popupCell.events as ev (ev.key)}
      <div class="cal-event">
        <span class="cal-event-dot" style:background={ev.color}></span>
        <span class="fw-500">{ev.popupLabel || ev.label}</span>
        {#if ev.detail}
          <span class="text-tertiary">{ev.detail}</span>
        {/if}
      </div>
    {/each}
    <svelte:fragment slot="footer">
      <span class="flex-1"></span>
      <button class="zf-btn" on:click={() => (popupCell = null)}
        >{$t("Close")}</button
      >
    </svelte:fragment>
  </Dialog>
{/if}

<style>
  /* Fixed width so the month name does not shift the arrow buttons around. */
  .cal-month-label {
    min-width: 70px;
  }

  .cal-card {
    padding: 16px;
  }

  .cal-legend {
    display: flex;
    gap: 8px;
    margin-top: 16px;
    flex-wrap: wrap;
  }

  .cal-legend-item {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 0.8125rem;
    padding: 6px 10px;
    border-radius: 4px;
    border: 1px solid var(--border);
    background: var(--bg-surface);
    cursor: pointer;
    transition: opacity 150ms ease-in-out, border-color 150ms ease-in-out, background-color 150ms ease-in-out;
  }

  .cal-legend-item:hover {
    border-color: var(--border-strong);
    background: var(--bg-muted);
  }

  .cal-legend-item.inactive {
    opacity: 0.45;
  }

  .cal-swatch {
    display: inline-block;
    width: 12px;
    height: 12px;
    border-radius: 2px;
    flex-shrink: 0;
  }

  .cal-event {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 0;
    font-size: 0.875rem;
  }

  .cal-event-dot {
    display: inline-block;
    width: 10px;
    height: 10px;
    border-radius: 2px;
    flex-shrink: 0;
  }
</style>
