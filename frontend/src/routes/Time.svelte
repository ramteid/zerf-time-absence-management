<script>
  import {
    categories,
    currentUser,
    path,
    go,
    toast,
    settings,
  } from "../stores.js";
  import { t, formatHours } from "../i18n.js";
  import { confirmDialog } from "../confirm.js";
  import {
    monday,
    addDays,
    isoDate,
    appTodayDate,
    appTodayIsoDate,
    dateKey,
    parseDate,
  } from "../format.js";
  import EntryDialog from "../dialogs/EntryDialog.svelte";
  import { isAssistantUser } from "../rolePolicy.js";
  import {
    getWeekData,
    requestWeekReopen,
    submitWeekEntries,
  } from "../lib/api/timeApi.js";
  import {
    isoWeekRange,
    sortByIsoDateAndStartTime,
    yearsInWeek,
  } from "../lib/domain/dates.js";
  import {
    buildBreakRules,
    buildWeekDays,
    computeDayBreakDeduction,
    creditedEntryMinutes,
    filterWeekAbsences,
    weekStatus as calculateWeekStatus,
    weekTargetMinutes as calculateWeekTargetMinutes,
  } from "../lib/domain/time.js";
  import TimeWeekHeader from "./time/TimeWeekHeader.svelte";
  import TimeWeekSummary from "./time/TimeWeekSummary.svelte";
  import WeekGrid from "./time/WeekGrid.svelte";
  import WeekendEntries from "./time/WeekendEntries.svelte";

  let entries = [];
  let absences = [];
  // The Monday that anchors the displayed week (weekFrom) and the Sunday that closes it (weekTo).
  let weekFrom, weekTo;
  let showEntry = null;
  let myReopens = [];
  // Monotonically increasing counter: any response whose sequence number is older than the
  // latest counter value is discarded, preventing stale async results from overwriting fresh data.
  let loadRequestCounter = 0;
  let weekdays = [];
  let weekendDays = [];
  let holidays = [];

  $: weekParam = (() => {
    const queryString = $path.includes("?") ? $path.split("?")[1] : "";
    // Close any open entry dialog when the week changes (URL-driven navigation).
    showEntry = null;
    return new URLSearchParams(queryString).get("week");
  })();
  $: requestedWeek = weekParam || appTodayIsoDate($settings?.timezone);
  $: timeFormat = $settings.time_format === "12h" ? "12h" : "24h";

  function setWeek(dateLike) {
    const weekStart = monday(
      parseDate(dateLike || appTodayDate($settings?.timezone)),
    );
    weekFrom = weekStart;
    weekTo = addDays(weekStart, 6);
    return weekStart;
  }

  async function loadWeek(dateLike = requestedWeek) {
    // Increment the counter so any in-flight responses from earlier loads are discarded.
    const requestId = ++loadRequestCounter;
    const weekStart = setWeek(dateLike);
    const { from, to } = isoWeekRange(weekStart);

    try {
      const {
        entries: weekEntries,
        reopenRows,
        categoryRows,
        absenceRowsByYear,
        holidayRowsByYear,
      } = await getWeekData({
        from,
        to,
        years: yearsInWeek(weekStart),
        fallbackCategories: $categories,
      });
      // Discard results from a superseded load – a newer request is already in flight.
      if (requestId !== loadRequestCounter) return;
      categories.set(categoryRows);
      entries = sortByIsoDateAndStartTime(weekEntries);
      myReopens = reopenRows;
      absences = filterWeekAbsences(absenceRowsByYear, from, to);
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
      const response = await submitWeekEntries(ids);
      if (response.auto_approved) {
        toast($t("Week approved."), "ok");
      } else {
        toast($t("Week submitted."), "ok");
      }
      await loadWeek(weekFrom || appTodayDate($settings?.timezone));
    } catch (error) {
      toast($t(error?.message || "Error"), "error");
    }
  }

  async function requestReopen() {
    if (!weekFrom) return;
    const reason = await confirmDialog(
      $t("Request edit for this week?"),
      $t(
        "Your team lead will be notified and must approve before the week becomes editable again.",
      ),
      { confirm: $t("Request edit"), reason: true },
    );
    if (!reason) return;
    try {
      const response = await requestWeekReopen(isoDate(weekFrom), reason);
      if (response.status === "auto_approved") {
        toast($t("Week editing enabled."), "ok");
      } else {
        toast($t("Edit request sent."), "ok");
      }
      await loadWeek(weekFrom || appTodayDate($settings?.timezone));
    } catch (error) {
      toast($t(error?.message || "Error"), "error");
    }
  }

  $: drafts = entries.filter((entry) => entry.status === "draft");
  $: isAssistantCurrentUser = isAssistantUser($currentUser);
  $: contractHours = formatHours($currentUser?.weekly_hours || 0);

  $: breakRules = buildBreakRules($settings);

  // Total logged minutes this week, excluding rejected entries, with per-day
  // automatic break deductions applied when the feature is enabled.
  // Break deductions are computed per calendar day (never spanning midnight),
  // then summed to produce the weekly total.
  $: weekLoggedMinutes = (() => {
    if (!breakRules.length) {
      // Fast path: no break feature — plain sum of credited entry minutes.
      return entries.reduce(
        (totalMinutes, entry) =>
          totalMinutes + creditedEntryMinutes(entry, $categories),
        0,
      );
    }
    // Group entries by their calendar date so each day's deduction is
    // calculated independently (break never spans midnight).
    const byDay = new Map();
    for (const entry of entries) {
      const dk = dateKey(entry.entry_date);
      if (!byDay.has(dk)) byDay.set(dk, []);
      byDay.get(dk).push(entry);
    }
    let total = 0;
    for (const dayEntries of byDay.values()) {
      const credited = dayEntries.reduce(
        (sum, e) => sum + creditedEntryMinutes(e, $categories),
        0,
      );
      const deduction = computeDayBreakDeduction(dayEntries, $categories, breakRules);
      total += Math.max(0, credited - deduction);
    }
    return total;
  })();

  // Weekly target is the sum of target-eligible weekdays in this week:
  // excludes holidays, absences, future days, and days before contract start.
  $: weekTargetMinutes = (() => {
    return calculateWeekTargetMinutes({
      weekdays,
      weekendDays,
      currentUser: $currentUser,
      todayIso: today,
    });
  })();

  $: weekLoggedHours = formatHours(weekLoggedMinutes / 60);
  $: weekTargetHours = formatHours(weekTargetMinutes / 60);
  $: weekHasTarget = !isAssistantCurrentUser && weekTargetMinutes > 0;

  $: {
    const builtWeek = weekFrom
      ? buildWeekDays(weekFrom, entries, absences, holidays)
      : { weekdays: [], weekendDays: [] };
    weekdays = builtWeek.weekdays;
    weekendDays = builtWeek.weekendDays;
  }

  // Insert or replace a single entry in the local list and re-sort.
  function upsertEntry(entry) {
    if (!entry) return;
    const otherEntries = entries.filter((existing) => existing.id !== entry.id);
    otherEntries.push(entry);
    entries = sortByIsoDateAndStartTime(otherEntries);
  }

  function removeEntry(id) {
    if (id == null) return;
    entries = entries.filter((entry) => entry.id !== id);
  }

  $: today = appTodayIsoDate($settings?.timezone);
  $: currentWeekMonday = monday(appTodayDate($settings?.timezone));
  // Disable the "next week" arrow once the user reaches the current week; looking
  // into the future is not allowed.
  $: isAtOrPastCurrentWeek =
    weekFrom && isoDate(weekFrom) >= isoDate(currentWeekMonday);

  $: weekStatus = calculateWeekStatus(entries, drafts);

  $: pendingReopen = (() => {
    if (!weekFrom) return null;
    const weekStartStr = isoDate(weekFrom);
    return (
      myReopens.find(
        (reopen) =>
          dateKey(reopen.week_start) === weekStartStr &&
          reopen.status === "pending",
      ) || null
    );
  })();

  // Reopen resets only submitted, approved, and rejected entries. Drafts can
  // coexist because they are already editable.
  $: canRequestReopen =
    !pendingReopen &&
    entries.some((entry) =>
      ["submitted", "approved", "rejected"].includes(entry.status),
    );

</script>

<TimeWeekHeader
  {weekFrom}
  {weekTo}
  isAssistant={isAssistantCurrentUser}
  {contractHours}
  {drafts}
  {canRequestReopen}
  {isAtOrPastCurrentWeek}
  on:prev={() => gotoWeek(-7)}
  on:next={() => gotoWeek(7)}
  on:submit={() => submitWeek(drafts.map((draft) => draft.id))}
  on:requestReopen={requestReopen}
/>

<div class="content-area">
  {#if weekFrom}
    <TimeWeekSummary
      {entries}
      {weekHasTarget}
      {weekLoggedMinutes}
      {weekTargetMinutes}
      {weekLoggedHours}
      {weekTargetHours}
      {pendingReopen}
      status={weekStatus}
    />

    <WeekGrid
      {weekdays}
      currentUser={$currentUser}
      categories={$categories}
      {weekStatus}
      {drafts}
      {timeFormat}
      {today}
      isAssistant={isAssistantCurrentUser}
      on:edit={(event) => (showEntry = event.detail)}
      on:add={(event) => (showEntry = event.detail)}
    />

    <WeekendEntries
      {weekendDays}
      currentUser={$currentUser}
      categories={$categories}
      {weekStatus}
      {timeFormat}
      on:edit={(event) => (showEntry = event.detail)}
    />
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
      loadWeek(weekFrom || appTodayDate($settings?.timezone));
    }}
  />
{/if}
