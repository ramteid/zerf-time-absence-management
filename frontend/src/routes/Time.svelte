<script>
  import { api } from "../api.js";
  import {
    categories,
    currentUser,
    path,
    go,
    toast,
    settings,
  } from "../stores.js";
  import { t, statusLabel, formatHours } from "../i18n.js";
  import { confirmDialog } from "../confirm.js";
  import {
    monday,
    addDays,
    isoDate,
    dateKey,
    parseDate,
    fmtDateShort,
    isoWeek,
    durMin,
    formatTimeValue,
  } from "../format.js";
  import Icon from "../Icons.svelte";
  import EntryDialog from "../dialogs/EntryDialog.svelte";
  import ChangeRequestDialog from "../dialogs/ChangeRequestDialog.svelte";

  // Full day names indexed Monday (0) → Sunday (6), used to label each day card.
  const WEEKDAY_NAMES = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
  ];

  let entries = [];
  let absences = [];
  // The Monday that anchors the displayed week (weekFrom) and the Sunday that closes it (weekTo).
  let weekFrom, weekTo;
  let showEntry = null;
  let showChange = null;
  let myReopens = [];
  // Monotonically increasing counter: any response whose sequence number is older than the
  // latest counter value is discarded, preventing stale async results from overwriting fresh data.
  let loadRequestCounter = 0;
  let weekdays = [];
  let weekendDays = [];
  let holidays = [];

  $: weekParam = (() => {
    const queryString = $path.includes("?") ? $path.split("?")[1] : "";
    return new URLSearchParams(queryString).get("week");
  })();
  $: requestedWeek = weekParam || isoDate(new Date());
  $: timeFormat = $settings.time_format === "12h" ? "12h" : "24h";

  function setWeek(dateLike) {
    const weekStart = monday(parseDate(dateLike || new Date()));
    weekFrom = weekStart;
    weekTo = addDays(weekStart, 6);
    return weekStart;
  }

  async function loadWeek(dateLike = requestedWeek) {
    // Increment the counter so any in-flight responses from earlier loads are discarded.
    const requestId = ++loadRequestCounter;
    const weekStart = setWeek(dateLike);
    const from = isoDate(weekStart);
    const to = isoDate(addDays(weekStart, 6));
    // A week can span two calendar years, so we may need holidays for both years.
    const yearsInWeek = Array.from(
      new Set([weekStart.getFullYear(), addDays(weekStart, 6).getFullYear()]),
    );

    try {
      const year = weekStart.getFullYear();
      const [
        weekEntries,
        reopenRows,
        categoryRows,
        absenceRows,
        holidayRowsByYear,
      ] = await Promise.all([
        api(`/time-entries?from=${from}&to=${to}`),
        api("/reopen-requests").catch(() => []),
        api("/categories").catch(() => $categories),
        api(`/absences?year=${year}`).catch(() => []),
        Promise.all(yearsInWeek.map((y) => api(`/holidays?year=${y}`).catch(() => []))),
      ]);
      // Discard results from a superseded load – a newer request is already in flight.
      if (requestId !== loadRequestCounter) return;
      categories.set(categoryRows);
      entries = [...weekEntries].sort((a, b) => {
        const dateDiff = dateKey(a.entry_date).localeCompare(dateKey(b.entry_date));
        if (dateDiff !== 0) return dateDiff;
        return a.start_time.localeCompare(b.start_time);
      });
      myReopens = reopenRows;
      absences = absenceRows.filter((absence) => absence.status !== "rejected");
      holidays = holidayRowsByYear.flat();
    } catch {
      if (requestId !== loadRequestCounter) return;
      entries = [];
      myReopens = [];
      absences = [];
      holidays = [];
    }
  }

  $: if ($path.startsWith("/time")) {
    loadWeek(requestedWeek);
  }

  function gotoWeek(offsetDays) {
    if (!weekFrom) return;
    const nextWeekStart = addDays(weekFrom, offsetDays);
    setWeek(nextWeekStart);
    entries = [];
    go("/time?week=" + isoDate(nextWeekStart));
  }

  async function submitWeek(ids) {
    if (!ids?.length) return;
    const ok = await confirmDialog(
      "Submit this week?",
      "All draft entries of this week will be submitted for approval.",
      { confirm: "Submit Week" },
    );
    if (!ok) return;
    try {
      await api("/time-entries/submit", { method: "POST", body: { ids } });
      toast($t("Week submitted."), "ok");
      await loadWeek(weekFrom || new Date());
    } catch (error) {
      toast($t(error?.message || "Error"), "error");
    }
  }

  async function requestReopen() {
    if (!weekFrom) return;
    const ok = await confirmDialog(
      $t("Reopen this week?"),
      $t(
        "Your team lead will be notified and must approve before the week becomes editable again.",
      ),
      { confirm: $t("Request edit") },
    );
    if (!ok) return;
    try {
      const response = await api("/reopen-requests", {
        method: "POST",
        body: { week_start: isoDate(weekFrom) },
      });
      if (response.status === "auto_approved") {
        toast($t("Week reopened."), "ok");
      } else {
        toast($t("Reopen request sent."), "ok");
      }
      await loadWeek(weekFrom || new Date());
    } catch (error) {
      toast($t(error?.message || "Error"), "error");
    }
  }

  // Returns the category object for a given ID, falling back to a placeholder when not found.
  function categoryById(categoryId, categoryRows) {
    return (
      categoryRows.find((category) => category.id === categoryId) || { name: "?", color: "#999" }
    );
  }

  $: drafts = entries.filter((entry) => entry.status === "draft");
  $: contractHours = formatHours($currentUser.weekly_hours || 0);

  // Total logged minutes this week, excluding rejected entries.
  $: weekActualMinutes = entries
    .filter((entry) => entry.status !== "rejected")
    .reduce(
      (totalMinutes, entry) =>
        entry.start_time && entry.end_time
          ? totalMinutes + durMin(entry.start_time.slice(0, 5), entry.end_time.slice(0, 5))
          : totalMinutes,
      0,
    );

  // Pro-rate the weekly target for the user's onboarding week: only count working
  // days from the contract start date through Friday. Weeks entirely before the
  // start date get a target of zero so no artificial deficit appears.
  $: effectiveWeeklyHours = (() => {
    const startDate = $currentUser?.start_date;
    const weekly = $currentUser?.weekly_hours || 0;
    if (!weekFrom || !startDate) return weekly;
    const weekStartStr = isoDate(weekFrom);
    const fridayStr = isoDate(addDays(weekFrom, 4));
    if (startDate > fridayStr) return 0;
    if (startDate >= weekStartStr) {
      const daysFromMonday = Math.round((parseDate(startDate) - weekFrom) / 86400000);
      return (Math.max(0, 5 - daysFromMonday) / 5) * weekly;
    }
    return weekly;
  })();

  $: weekLoggedHours = formatHours((weekActualMinutes / 60).toFixed(1));
  $: weekTargetHours = formatHours(effectiveWeeklyHours.toFixed(1));

  // Build a descriptor for one day of the week. All data is passed explicitly so
  // that Svelte's reactive system can track the dependencies correctly.
  function buildWeekDay(dayIndex, entryRows, absenceRows, holidayRows) {
    const dayDate = addDays(weekFrom, dayIndex);
    const dayDateStr = isoDate(dayDate);
    return {
      d: dayDate,
      ds: dayDateStr,
      dayName: WEEKDAY_NAMES[dayIndex],
      absent: absenceRows.some(
        (absence) => absence.start_date <= dayDateStr && absence.end_date >= dayDateStr,
      ),
      holiday: holidayRows.some((holiday) => holiday.holiday_date === dayDateStr),
      items: entryRows
        .filter((entry) => dateKey(entry.entry_date) === dayDateStr)
        .sort((a, b) => a.start_time.localeCompare(b.start_time)),
    };
  }

  $: weekdays = weekFrom
    ? [0, 1, 2, 3, 4].map((dayIndex) => buildWeekDay(dayIndex, entries, absences, holidays))
    : [];
  $: weekendDays = weekFrom
    ? [5, 6].map((dayIndex) => buildWeekDay(dayIndex, entries, absences, holidays))
    : [];

  function entryDurationHours(startTime, endTime) {
    return (durMin(startTime, endTime) / 60).toFixed(1);
  }

  function formatDisplayTime(rawTimeValue, format) {
    return formatTimeValue(rawTimeValue?.slice(0, 5) || "", format);
  }

  function entryTimeRange(entry, format) {
    return `${formatDisplayTime(entry.start_time, format)} - ${formatDisplayTime(
      entry.end_time,
      format,
    )}`;
  }

  // Insert or replace a single entry in the local list and re-sort.
  function upsertEntry(entry) {
    if (!entry) return;
    const otherEntries = entries.filter((existing) => existing.id !== entry.id);
    otherEntries.push(entry);
    entries = otherEntries.sort((a, b) => {
      const dateDiff = dateKey(a.entry_date).localeCompare(dateKey(b.entry_date));
      if (dateDiff !== 0) return dateDiff;
      return a.start_time.localeCompare(b.start_time);
    });
  }

  function removeEntry(id) {
    if (id == null) return;
    entries = entries.filter((entry) => entry.id !== id);
  }

  $: today = isoDate(new Date());
  $: currentWeekMonday = monday(new Date());
  // Disable the "next week" arrow once the user reaches the current week; looking
  // into the future is not allowed.
  $: isAtOrPastCurrentWeek = weekFrom && isoDate(weekFrom) >= isoDate(currentWeekMonday);

  // Maps a week status to a CSS color variable so the stat card text is
  // immediately readable without needing a chip: red for actionable states
  // (draft = still needs work, rejected = needs correction), green for
  // positive outcomes (submitted = awaiting approval, approved = done),
  // and orange for mixed states (partial = some approved, some rejected).
  function weekStatusColor(status) {
    switch (status) {
      case "draft":     return "var(--danger-text)";
      case "submitted": return "var(--success-text)";
      case "approved":  return "var(--success-text)";
      case "rejected":  return "var(--danger-text)";
      case "partial":   return "var(--warning-text)";
      default:          return "var(--text-primary)";
    }
  }

  $: weekStatus = (() => {
    if (entries.length === 0) return "draft";
    const nonDraftEntries = entries.filter((entry) => entry.status !== "draft");
    if (nonDraftEntries.length === 0) return "draft";
    if (nonDraftEntries.every((entry) => entry.status === "approved")) return "approved";
    if (nonDraftEntries.some((entry) => entry.status === "submitted")) return "submitted";
    if (nonDraftEntries.every((entry) => entry.status === "rejected")) return "rejected";
    // Mix of approved + rejected with nothing pending: surface as "partial" so
    // the user knows there are rejected entries without hiding the approvals.
    return "partial";
  })();

  $: pendingReopen = (() => {
    if (!weekFrom) return null;
    const weekStartStr = isoDate(weekFrom);
    return (
      myReopens.find(
        (reopen) =>
          dateKey(reopen.week_start) === weekStartStr && reopen.status === "pending",
      ) || null
    );
  })();

  // Show the reopen button only when the week is fully submitted (no remaining drafts)
  // and has at least one non-draft entry – but not while a reopen request is pending.
  $: canRequestReopen =
    !pendingReopen &&
    !drafts.length &&
    entries.some((entry) => entry.status !== "draft");

  // The add-entry button is shown on all weekdays to keep the layout consistent,
  // but disabled on days where adding time entries makes no sense.
  function isDayAddDisabled(day) {
    return (
      day.absent ||
      day.holiday ||
      day.ds > today ||
      ($currentUser?.start_date && day.ds < $currentUser.start_date)
    );
  }
</script>

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t("Time Entry")}</h1>
  </div>
  {#if weekFrom}
    <div class="top-bar-subtitle">
      {$t("Week {week}", { week: isoWeek(weekFrom) })} · {contractHours}
      {$t("contract")}
    </div>
  {/if}
  <div class="top-bar-actions time-top-bar-actions">
    {#if weekFrom}
      <div class="time-week-picker" style="display:flex;align-items:center;gap:4px">
        <button
          class="kz-btn kz-btn-icon-sm kz-btn-ghost"
          on:click={() => gotoWeek(-7)}
        >
          <Icon name="ChevLeft" size={16} />
        </button>
        <span
          class="tab-num time-week-label"
          style="font-size:13.5px;font-weight:500;text-align:center"
        >
          {fmtDateShort(weekFrom)} – {fmtDateShort(weekTo)}
        </span>
        <button
          class="kz-btn kz-btn-icon-sm kz-btn-ghost"
          on:click={() => gotoWeek(7)}
          disabled={isAtOrPastCurrentWeek}
        >
          <Icon name="ChevRight" size={16} />
        </button>
      </div>
    {/if}

    <button
      class="kz-btn kz-btn-primary time-submit-button"
      on:click={() => submitWeek(drafts.map((draft) => draft.id))}
      disabled={!drafts.length}
    >
      <Icon name="Send" size={14} />{$t("Submit Week")}
    </button>

    {#if canRequestReopen}
      <button
        class="kz-btn kz-btn-sm"
        on:click={requestReopen}
        title={$t("Request edit")}
      >
        <Icon name="Edit" size={13} />{$t("Request edit")}
      </button>
    {/if}
  </div>
</div>

<div class="content-area">
  {#if weekFrom}
    {#if entries.length > 0}
      <!-- Summary strip: rendered only once there is something to summarise. -->
      <div class="stat-cards" style="margin-bottom:16px">
        <div class="kz-card stat-card">
          <div class="stat-card-label">{$t("Logged")}</div>
          <div
            class="stat-card-value tab-num"
            style="color: {weekActualMinutes >= effectiveWeeklyHours * 60
              ? 'var(--accent)'
              : 'var(--warning-text)'}"
          >
            {weekLoggedHours}
          </div>
          <div class="stat-card-sub">
            {$t("of {target} target", { target: weekTargetHours })}
          </div>
        </div>
        <div class="kz-card stat-card">
          <div class="stat-card-label">{$t("Status")}</div>
          <div
            class="stat-card-value tab-num"
            style="font-size:18px;color:{weekStatusColor(weekStatus)}"
          >
            {statusLabel(weekStatus)}
          </div>
        </div>
      </div>
    {/if}

    <!-- Week grid: one card per weekday (Mon–Fri) -->
    <div class="week-grid">
      {#each weekdays as day (day.ds)}
        {@const dailyTargetHours = ($currentUser.weekly_hours || 0) / 5}
        {@const dailyTotalMinutes = day.items.reduce(
          (totalMinutes, entry) =>
            entry.status === "rejected"
              ? totalMinutes
              : totalMinutes + durMin(entry.start_time.slice(0, 5), entry.end_time.slice(0, 5)),
          0,
        )}
        {@const dailyTotalHours = (dailyTotalMinutes / 60).toFixed(1)}
        <div
          class="kz-card day-card"
          class:day-card--locked={weekStatus === "submitted" ||
            weekStatus === "approved"}
          class:day-card--absent={day.absent}
          class:day-card--before-start={$currentUser?.start_date &&
            day.ds < $currentUser.start_date}
        >
          <div class="day-header">
            <div>
              <div class="day-name">{$t(day.dayName)}</div>
              <div class="day-date tab-num">{fmtDateShort(day.d)}</div>
            </div>
            <div
              class="day-total tab-num"
              style="color: {dailyTotalMinutes / 60 >= dailyTargetHours
                ? 'var(--accent)'
                : 'var(--text-primary)'}"
            >
              {formatHours(dailyTotalHours)}
            </div>
          </div>

          <div class="day-entries">
            {#each day.items as entry}
              {@const category = categoryById(entry.category_id, $categories)}
              <div
                class="time-block"
                class:time-block--rejected={entry.status === "rejected"}
                on:click={() => {
                  if (entry.status === "draft") showEntry = entry;
                  else if (entry.status === "submitted" || entry.status === "approved")
                    showChange = entry;
                }}
                on:keydown={() => {}}
                role="button"
                tabindex="0"
              >
                <div class="time-block-cat">
                  <span class="cat-dot" style="background:{category.color}"></span>
                  <span class="time-block-cat-name">{$t(category.name)}</span>
                  {#if entry.status !== "draft"}
                    <span
                      class="kz-chip kz-chip-{entry.status}"
                      style="height:18px;font-size:10px"
                    >
                      {statusLabel(entry.status)}
                    </span>
                  {/if}
                </div>
                <div class="time-block-times tab-num">
                  <span>{entryTimeRange(entry, timeFormat)}</span>
                  <span
                    >{formatHours(
                      entryDurationHours(
                        entry.start_time.slice(0, 5),
                        entry.end_time.slice(0, 5),
                      ),
                    )}</span
                  >
                </div>
              </div>
            {/each}
          </div>

          {#if weekStatus === "draft" || drafts.length > 0}
            <div class="day-add-btn">
              <button
                class="kz-btn kz-btn-ghost kz-btn-sm"
                style="width:100%;justify-content:center;border-style:dashed;border-color:var(--border)"
                disabled={isDayAddDisabled(day)}
                on:click={() => (showEntry = { entry_date: day.ds })}
              >
                <Icon name="Plus" size={13} />{$t("Add")}
              </button>
            </div>
          {/if}
        </div>
      {/each}
    </div>

    <!-- Weekend cards (Sat/Sun): only rendered when entries exist on those days -->
    {#each weekendDays as day (day.ds)}
      {#if day.items.length > 0}
        <div class="kz-card" style="margin-top:12px;overflow-x:auto">
          <div class="day-header">
            <div>
              <div class="day-name">{$t(day.dayName)}</div>
              <div class="day-date tab-num">{fmtDateShort(day.d)}</div>
            </div>
          </div>
          <div class="day-entries">
            {#each day.items as entry}
              {@const category = categoryById(entry.category_id, $categories)}
              <div class="time-block" class:time-block--rejected={entry.status === "rejected"}>
                <div class="time-block-cat">
                  <span class="cat-dot" style="background:{category.color}"></span>
                  <span class="time-block-cat-name">{$t(category.name)}</span>
                </div>
                <div class="time-block-times tab-num">
                  <span>{entryTimeRange(entry, timeFormat)}</span>
                </div>
              </div>
            {/each}
          </div>
        </div>
      {/if}
    {/each}
  {/if}
</div>

{#if showEntry}
  <EntryDialog
    template={showEntry}
    onClose={({ changed, entry, deletedId }) => {
      showEntry = null;
      if (!changed) return;
      removeEntry(deletedId);
      upsertEntry(entry);
      loadWeek(weekFrom || new Date());
    }}
  />
{/if}
{#if showChange}
  <ChangeRequestDialog
    entry={showChange}
    onClose={() => {
      showChange = null;
      loadWeek(weekFrom || new Date());
    }}
  />
{/if}

<style>
  .time-block--rejected .time-block-cat-name,
  .time-block--rejected .time-block-times {
    text-decoration: line-through;
    color: var(--text-tertiary);
  }

  .day-card--before-start {
    opacity: 0.4;
  }

</style>
