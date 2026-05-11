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
  import { t, statusLabel, formatHours, absenceKindLabel } from "../i18n.js";
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

  const ABSENCE_COLORS = Object.freeze({
    vacation: "#3b82f6",
    sick: "#ef4444",
    training: "#0d9488",
    special_leave: "#d97706",
    unpaid: "#6b7280",
    general_absence: "#475569",
  });

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
    const weekEndIsoDate = isoDate(addDays(weekStart, 6));
    // A week can span two calendar years, so we may need holidays for both years.
    const yearsInWeek = Array.from(
      new Set([weekStart.getFullYear(), addDays(weekStart, 6).getFullYear()]),
    );

    try {
      const [
        weekEntries,
        reopenRows,
        categoryRows,
        absenceRowsByYear,
        holidayRowsByYear,
      ] = await Promise.all([
        api(`/time-entries?from=${from}&to=${weekEndIsoDate}`),
        api("/reopen-requests").catch(() => []),
        api("/categories").catch(() => $categories),
        Promise.all(
          yearsInWeek.map((yearValue) =>
            api(`/absences?year=${yearValue}`).catch(() => []),
          ),
        ),
        Promise.all(yearsInWeek.map((yearValue) => api(`/holidays?year=${yearValue}`).catch(() => []))),
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
      const seenAbsenceIds = new Set();
      absences = absenceRowsByYear
        .flat()
        .filter((absence) => {
          if (seenAbsenceIds.has(absence.id)) return false;
          seenAbsenceIds.add(absence.id);
          return (
            absence.end_date >= from &&
            absence.start_date <= weekEndIsoDate &&
            absence.status !== "rejected" &&
            absence.status !== "cancelled"
          );
        });
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
    const confirmed = await confirmDialog(
      $t("Submit this week?"),
      $t("All draft entries of this week will be submitted for approval."),
      { confirm: $t("Submit Week") },
    );
    if (!confirmed) return;
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
    const confirmed = await confirmDialog(
      $t("Reopen this week?"),
      $t(
        "Your team lead will be notified and must approve before the week becomes editable again.",
      ),
      { confirm: $t("Request edit") },
    );
    if (!confirmed) return;
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
  $: weekLoggedMinutes = entries
    .filter((entry) => entry.status !== "rejected")
    .reduce(
      (totalMinutes, entry) =>
        entry.start_time && entry.end_time
          ? totalMinutes + durMin(entry.start_time.slice(0, 5), entry.end_time.slice(0, 5))
          : totalMinutes,
      0,
    );

  // Backend monthly "actual" uses approved entries only; keep weekly summary aligned.
  $: weekApprovedMinutes = entries
    .filter((entry) => entry.status === "approved")
    .reduce(
      (totalMinutes, entry) =>
        entry.start_time && entry.end_time
          ? totalMinutes + durMin(entry.start_time.slice(0, 5), entry.end_time.slice(0, 5))
          : totalMinutes,
      0,
    );

  // Weekly target is the sum of target-eligible weekdays in this week:
  // excludes holidays, absences, future days, and days before contract start.
  $: weekTargetMinutes = (() => {
    const weeklyHours = Number($currentUser?.weekly_hours || 0);
    const workdaysPerWeek = Number($currentUser?.workdays_per_week || 5);
      // Calculate daily target based on user's workdays_per_week configuration.
      // Example: 40h/week ÷ 5 days = 8h/day = 480 min/day (5-day worker)
      //          40h/week ÷ 4 days = 10h/day = 600 min/day (4-day worker)
    const perDayMinutes = Math.round((weeklyHours / workdaysPerWeek) * 60);
    if (perDayMinutes <= 0) return 0;
    return weekdays.reduce((totalMinutes, day) => {
      const isBeforeStart = $currentUser?.start_date && day.ds < $currentUser.start_date;
      const isFuture = day.ds > today;
      if (day.absentForTarget || day.holiday || isBeforeStart || isFuture) {
        return totalMinutes;
      }
      return totalMinutes + perDayMinutes;
    }, 0);
  })();

  $: weekLoggedHours = formatHours((weekLoggedMinutes / 60).toFixed(1));
  $: weekApprovedHours = formatHours((weekApprovedMinutes / 60).toFixed(1));
  $: weekTargetHours = formatHours((weekTargetMinutes / 60).toFixed(1));

  // Build a descriptor for one day of the week. All data is passed explicitly so
  // that Svelte's reactive system can track the dependencies correctly.
  function buildWeekDay(dayIndex, entryRows, absenceRows, holidayRows) {
    const dayDate = addDays(weekFrom, dayIndex);
    const dayDateStr = isoDate(dayDate);
    const matchingAbsence = absenceRows.find(
      (absence) => absence.start_date <= dayDateStr && absence.end_date >= dayDateStr,
    );
    const matchingHoliday = holidayRows.find((holiday) => holiday.holiday_date === dayDateStr);
    return {
      d: dayDate,
      ds: dayDateStr,
      dayName: WEEKDAY_NAMES[dayIndex],
      absent: !!matchingAbsence,
      // Keep day-level target rules aligned with backend reports:
      // only approved/cancellation_pending absences remove daily target.
      absentForTarget: matchingAbsence
        ? ["approved", "cancellation_pending"].includes(matchingAbsence.status)
        : false,
      holiday: !!matchingHoliday,
      absenceKind: matchingAbsence?.kind || null,
      holidayName: matchingHoliday?.name || null,
      items: entryRows
        .filter((entry) => dateKey(entry.entry_date) === dayDateStr)
        .sort((a, b) => a.start_time.localeCompare(b.start_time)),
    };
  }

  function absenceColor(kind) {
    return ABSENCE_COLORS[kind] || "var(--text-tertiary)";
  }

  $: weekdays = weekFrom
      // Dynamic weekday grid: adapt to user's workdays_per_week setting.
      // E.g., 5-day worker sees Mon-Fri, 4-day worker sees Mon-Thu.
    ? Array.from({ length: $currentUser?.workdays_per_week || 5 }, (_, i) => i)
        .map((dayIndex) => buildWeekDay(dayIndex, entries, absences, holidays))
    : [];
  $: weekendDays = weekFrom
      // Non-contract days shown separately (e.g., Sat-Sun for 5-day worker, Fri-Sun for 4-day).
    ? Array.from({ length: 7 - ($currentUser?.workdays_per_week || 5) }, (_, i) => ($currentUser?.workdays_per_week || 5) + i)
        .map((dayIndex) => buildWeekDay(dayIndex, entries, absences, holidays))
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
    if (nonDraftEntries.length === entries.length && nonDraftEntries.every((entry) => entry.status === "approved")) return "approved";
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
      day.absentForTarget ||
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
      <div class="kz-nav-slider time-week-picker">
        <button
          class="kz-btn kz-btn-ghost"
          on:click={() => gotoWeek(-7)}
        >
          <Icon name="ChevLeft" size={16} />
        </button>
        <span class="nav-label tab-num time-week-label">
          {fmtDateShort(weekFrom)} – {fmtDateShort(weekTo)}
        </span>
        <button
          class="kz-btn kz-btn-ghost"
          on:click={() => gotoWeek(7)}
          disabled={isAtOrPastCurrentWeek}
        >
          <Icon name="ChevRight" size={16} />
        </button>
      </div>
    {/if}

    <!-- Submit and reopen buttons stacked vertically. -->
    <div class="time-submit-stack">
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
</div>

<div class="content-area">
  {#if weekFrom}
    {#if entries.length > 0}
      <!-- Summary strip: rendered only once there is something to summarise. -->
      <div class="stat-cards" style="margin-bottom:16px">
        <div class="kz-card stat-card">
          <div class="stat-card-label">{$t("Approved")}</div>
          <div
            class="stat-card-value tab-num"
            style="color: {weekApprovedMinutes >= weekTargetMinutes
              ? 'var(--accent)'
              : 'var(--warning-text)'}"
          >
            {weekApprovedHours}
          </div>
          <div class="stat-card-sub">
            {$t("of {target} target", { target: weekTargetHours })} - {$t("Logged")}: {weekLoggedHours}
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
        {@const dailyTargetHours = ($currentUser.weekly_hours || 0) / ($currentUser.workdays_per_week || 5)}
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
            {#if day.absenceKind || day.holiday}
              {@const statusColor = day.absenceKind
                ? absenceColor(day.absenceKind)
                : "var(--warning-text)"}
              <div class="day-status-indicator" style={`--status-color:${statusColor}`}>
                <span class="day-status-dot" aria-hidden="true"></span>
                <span class="day-status-text"
                  >{day.absenceKind
                    ? absenceKindLabel(day.absenceKind)
                    : day.holidayName || $t("Public holiday")}</span
                >
              </div>
            {/if}

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

  .day-status-indicator {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    align-self: center;
    gap: 8px;
    margin: auto;
    max-width: 100%;
    padding: 6px 10px;
    border-radius: 999px;
    border: 1px solid color-mix(in srgb, var(--status-color) 28%, transparent);
    background: color-mix(in srgb, var(--status-color) 12%, transparent);
    color: var(--status-color);
    font-size: 12px;
    font-weight: 600;
    text-align: center;
  }

  .day-status-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
    background: var(--status-color);
  }

  .day-status-text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

</style>
