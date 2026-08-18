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
    compareEventGroups,
    groupDayEvents,
    pruneCategoryFilter,
    rawCellEvents,
    toggleCategoryFilter,
  } from "../lib/domain/calendar.js";
  import CategoryFilter from "./calendar/CategoryFilter.svelte";
  import { tracksOwnTime } from "../rolePolicy.js";

  let entries = [];
  let holidays = [];
  let timeEntries = [];
  let users = [];
  let year, month;
  // eslint-disable-next-line no-useless-assignment
  let popupCell = null;
  let loadSeq = 0;
  // Category filter: the colorKeys the viewer has hidden. Empty = show all.
  let hiddenCategories = new Set();

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
  // Own time entries carry only a user id; the /users lookup is empty for
  // employees, so the viewer's own name comes from the session instead.
  $: currentUserName =
    `${$currentUser?.first_name ?? ""} ${$currentUser?.last_name ?? ""}`.trim() ||
    null;
  $: calendarContext = {
    entryMap: teMap,
    categoryMap: categoryById,
    absenceCategoryMap: absCatBySlug,
    translate: $t,
    userMap: userById,
    currentUserId: $currentUser?.id ?? null,
    currentUserName,
  };
  $: colorByKey = buildColorMap(cells, calendarContext);
  $: eventCells = cells.map((cell) => {
    const visibleEvents = cellEvents(cell, calendarContext, colorByKey).filter(
      (event) => !hiddenCategories.has(event.colorKey),
    );
    // One group per category: the day cell shows a single chip per category
    // and the popup lists every record inside it.
    return { ...cell, groups: groupDayEvents(visibleEvents) };
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
    (cell) => cell.weekend && cell.groups.length > 0,
  );
  $: visibleWeekdayLabels = showWeekends
    ? weekdayLabels()
    : weekdayLabels().slice(0, 5);
  $: visibleEventCells = showWeekends
    ? eventCells
    : eventCells.filter((cell) => !cell.weekend);
  $: calGridColumns = showWeekends ? 7 : 5;

  // Every category the month contains, filter state aside — the filter menu is
  // built from this, so hiding a category never removes it from the menu and
  // leaves the viewer with no way back. Order follows the same holiday →
  // absence → work-category ranking the day cells and the popup use, so a
  // category never moves around between views.
  $: categoryItems = (() => {
    const seen = new Map();
    for (const cell of cells) {
      if (cell.other) continue;
      for (const event of rawCellEvents(cell, calendarContext)) {
        if (seen.has(event.colorKey)) continue;
        seen.set(event.colorKey, {
          colorKey: event.colorKey,
          color: colorByKey.get(event.colorKey) || event.color,
          label: event.label,
        });
      }
    }
    return [...seen.values()].sort(compareEventGroups);
  })();
  $: categoryKeys = categoryItems.map((item) => item.colorKey);

  // Navigating to a month without one of the hidden categories drops it from
  // the filter. The identity check keeps this from re-assigning (and so
  // re-running) on every render when there is nothing to drop.
  $: {
    const pruned = pruneCategoryFilter(hiddenCategories, categoryKeys);
    if (pruned !== hiddenCategories) hiddenCategories = pruned;
  }

  function toggleCategory(colorKey) {
    hiddenCategories = toggleCategoryFilter(
      hiddenCategories,
      colorKey,
      categoryKeys,
    );
  }

  function showAllCategories() {
    hiddenCategories = new Set();
  }

  function hideAllCategories() {
    hiddenCategories = new Set(categoryKeys);
  }

  function clickDay(cell) {
    if (cell.groups.length === 0) return;
    popupCell = cell;
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
    <CategoryFilter
      items={categoryItems}
      hidden={hiddenCategories}
      onToggle={toggleCategory}
      onShowAll={showAllCategories}
      onHideAll={hideAllCategories}
    />
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
        {@const groups = c.groups}
        {#if c.weekend && groups.length === 0}
          <!-- Keeps the grid slot so the shared 7-column grid stays aligned,
               without showing a visible box for a weekend day with no
               entries. Saturday and Sunday are hidden independently: one
               can be shown while the other stays hidden in the same week. -->
          <div class="cal-day-spacer" aria-hidden="true"></div>
        {:else}
          <button
            type="button"
            class="cal-day"
            data-date={c.ds}
            class:has-events={groups.length > 0}
            class:today={c.today}
            class:weekend={c.weekend && !c.today}
            class:other-month={c.other}
            style:border-left={groups.length
              ? `3px solid ${groups[0].color}`
              : null}
            on:click={() => clickDay(c)}
            disabled={groups.length === 0}
          >
            <div class="cal-day-number tab-num">{c.d.getDate()}</div>
            {#if groups.length}
              <div class="cal-events">
                <!-- One chip per category, never one per record: six people on
                     vacation show a single "Vacation" chip carrying the count. -->
                {#each groups.slice(0, 3) as group (group.key)}
                  <div class="cal-event" style:background={group.color}>
                    <span class="cal-event-title"
                      >{calendarEventTitle(group)}</span
                    >
                    {#if group.count > 1}
                      <span class="cal-event-count tab-num">{group.count}</span>
                    {/if}
                  </div>
                {/each}
                {#if groups.length > 3}
                  <div class="cal-more">+{groups.length - 3}</div>
                {/if}
              </div>
            {/if}
          </button>
        {/if}
      {/each}
    </div>
  </div>
</div>

{#if popupCell}
  <Dialog title={fmtDate(popupCell.ds)} onClose={() => (popupCell = null)}>
    <!-- Every day opens this same view: one block per category, and inside it
         one row per record, so the layout never depends on which chip was
         clicked or on how many people share a category. -->
    <div class="cal-popup">
      {#each popupCell.groups as group (group.key)}
        <div class="cal-popup-group">
          <div class="cal-popup-group-head">
            <span class="cal-popup-dot" style:background={group.color}></span>
            <span class="fw-500">{group.label}</span>
          </div>
          <div class="cal-popup-rows">
            {#each group.items as item (item.key)}
              <div class="cal-popup-row">
                <span class="cal-popup-primary">{item.primary}</span>
                {#if item.secondary}
                  <span class="cal-popup-secondary text-tertiary"
                    >{item.secondary}</span
                  >
                {/if}
              </div>
            {/each}
          </div>
        </div>
      {/each}
    </div>
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

  /* Day popup. Indentation is fixed rather than content-derived: the rows of
     every group start at the same offset (swatch width + gap) no matter how
     long the category name or a person's name is. */
  .cal-popup {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .cal-popup-group-head {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 0.875rem;
  }

  .cal-popup-dot {
    display: inline-block;
    width: 10px;
    height: 10px;
    border-radius: 2px;
    flex-shrink: 0;
  }

  .cal-popup-rows {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-top: 4px;
    padding-left: 18px;
  }

  .cal-popup-row {
    display: flex;
    align-items: baseline;
    gap: 12px;
    font-size: 0.875rem;
  }

  .cal-popup-primary {
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .cal-popup-secondary {
    margin-left: auto;
    flex-shrink: 0;
    white-space: nowrap;
  }
</style>
