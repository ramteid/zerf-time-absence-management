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

  const DAYS_FULL = [
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
  let mo, su;
  let showEntry = null;
  let showChange = null;
  let myReopens = [];
  let loadSeq = 0;
  let weekdays = [];
  let weekendDays = [];

  $: weekParam = (() => {
    const q = $path.includes("?") ? $path.split("?")[1] : "";
    return new URLSearchParams(q).get("week");
  })();
  $: requestedWeek = weekParam || isoDate(new Date());
  $: timeFormat = $settings.time_format === "12h" ? "12h" : "24h";

  function setWeek(dateLike) {
    const start = monday(parseDate(dateLike || new Date()));
    mo = start;
    su = addDays(start, 6);
    return start;
  }

  async function loadWeek(dateLike = requestedWeek) {
    const seq = ++loadSeq;
    const start = setWeek(dateLike);
    const from = isoDate(start);
    const to = isoDate(addDays(start, 6));

    try {
      const year = start.getFullYear();
      const [weekEntries, reopenRows, categoryRows, absenceRows] = await Promise.all([
        api(`/time-entries?from=${from}&to=${to}`),
        api("/reopen-requests").catch(() => []),
        api("/categories").catch(() => $categories),
        api(`/absences?year=${year}`).catch(() => []),
      ]);
      if (seq !== loadSeq) return;
      categories.set(categoryRows);
      entries = [...weekEntries].sort((a, b) => {
        const byDate = dateKey(a.entry_date).localeCompare(
          dateKey(b.entry_date),
        );
        if (byDate !== 0) return byDate;
        return a.start_time.localeCompare(b.start_time);
      });
      myReopens = reopenRows;
      absences = absenceRows.filter((a) => a.status !== "rejected");
    } catch {
      if (seq !== loadSeq) return;
      entries = [];
      myReopens = [];
      absences = [];
    }
  }

  $: if ($path.startsWith("/time")) {
    loadWeek(requestedWeek);
  }

  function gotoWeek(offset) {
    if (!mo) return;
    const next = addDays(mo, offset);
    setWeek(next);
    entries = [];
    go("/time?week=" + isoDate(next));
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
      await loadWeek(mo || new Date());
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
    }
  }

  async function requestReopen() {
    if (!mo) return;
    const ok = await confirmDialog(
      $t("Reopen this week?"),
      $t(
        "Your team lead will be notified and must approve before the week becomes editable again.",
      ),
      { confirm: $t("Request edit") },
    );
    if (!ok) return;
    try {
      const r = await api("/reopen-requests", {
        method: "POST",
        body: { week_start: isoDate(mo) },
      });
      if (r.status === "auto_approved") {
        toast($t("Week reopened."), "ok");
      } else {
        toast($t("Reopen request sent."), "ok");
      }
      await loadWeek(mo || new Date());
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
    }
  }

  function catOf(id, categoryRows) {
    return (
      categoryRows.find((c) => c.id === id) || { name: "?", color: "#999" }
    );
  }

  $: weekActual = entries
    .filter((e) => e.status !== "rejected")
    .reduce(
      (s, e) =>
        s +
        (e.start_time && e.end_time
          ? durMin(e.start_time.slice(0, 5), e.end_time.slice(0, 5))
          : 0),
      0,
    );
  $: drafts = entries.filter((e) => e.status === "draft");
  $: weekHours = (weekActual / 60).toFixed(1);
  $: contractHours = formatHours($currentUser.weekly_hours || 0);
  $: targetHours = formatHours(($currentUser.weekly_hours || 0).toFixed(1));
  $: overtime = Math.max(
    0,
    weekActual / 60 - ($currentUser.weekly_hours || 0),
  ).toFixed(1);
  $: remaining = Math.max(
    0,
    ($currentUser.weekly_hours || 0) - weekActual / 60,
  ).toFixed(1);

  function hasAbsence(ds) {
    return absences.some((a) => a.start_date <= ds && a.end_date >= ds);
  }

  function buildDay(i, entryRows) {
    const d = addDays(mo, i);
    const ds = isoDate(d);
    return {
      d,
      ds,
      dayName: DAYS_FULL[i],
      absent: hasAbsence(ds),
      items: entryRows
        .filter((e) => dateKey(e.entry_date) === ds)
        .sort((a, b) => a.start_time.localeCompare(b.start_time)),
    };
  }

  $: weekdays = mo ? [0, 1, 2, 3, 4].map((i) => buildDay(i, entries, absences)) : [];
  $: weekendDays = mo ? [5, 6].map((i) => buildDay(i, entries, absences)) : [];

  function durHours(start, end) {
    return (durMin(start, end) / 60).toFixed(1);
  }

  function displayTime(value, format) {
    return formatTimeValue(value?.slice(0, 5) || "", format);
  }

  function entryTimeRange(entry, format) {
    return `${displayTime(entry.start_time, format)} - ${displayTime(
      entry.end_time,
      format,
    )}`;
  }

  function upsertEntry(entry) {
    if (!entry) return;
    const next = entries.filter((item) => item.id !== entry.id);
    next.push(entry);
    entries = next.sort((a, b) => {
      const byDate = dateKey(a.entry_date).localeCompare(dateKey(b.entry_date));
      if (byDate !== 0) return byDate;
      return a.start_time.localeCompare(b.start_time);
    });
  }

  function removeEntry(id) {
    if (id == null) return;
    entries = entries.filter((entry) => entry.id !== id);
  }

  $: today = isoDate(new Date());
  $: currentWeekMo = monday(new Date());
  $: isCurrentWeek = mo && isoDate(mo) >= isoDate(currentWeekMo);

  $: weekStatus = (() => {
    if (entries.length === 0) return "draft";
    const nonDraft = entries.filter((e) => e.status !== "draft");
    if (nonDraft.length === 0) return "draft";
    if (nonDraft.every((e) => e.status === "approved")) return "approved";
    if (nonDraft.some((e) => e.status === "submitted")) return "submitted";
    if (nonDraft.every((e) => e.status === "rejected")) return "rejected";
    // Mix of approved + rejected with nothing pending: surface as "partial" so
    // the user knows there are rejected entries without hiding the approvals.
    return "partial";
  })();

  $: pendingReopen = (() => {
    if (!mo) return null;
    const ws = isoDate(mo);
    return (
      myReopens.find(
        (r) => dateKey(r.week_start) === ws && r.status === "pending",
      ) || null
    );
  })();
  $: canRequestReopen =
    !pendingReopen &&
    !drafts.length &&
    entries.some((e) => e.status !== "draft");
</script>

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t("Time Entry")}</h1>
  </div>
  {#if mo}
    <div class="top-bar-subtitle">
      {$t("Week {week}", { week: isoWeek(mo) })} · {contractHours}
      {$t("contract")}
    </div>
  {/if}
  <div class="top-bar-actions time-top-bar-actions">
    {#if mo}
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
          {fmtDateShort(mo)} – {fmtDateShort(su)}
        </span>
        <button
          class="kz-btn kz-btn-icon-sm kz-btn-ghost"
          on:click={() => gotoWeek(7)}
          disabled={isCurrentWeek}
        >
          <Icon name="ChevRight" size={16} />
        </button>
      </div>
    {/if}
    <button
      class="kz-btn kz-btn-primary time-submit-button"
      on:click={() => submitWeek(drafts.map((x) => x.id))}
      disabled={!drafts.length}
    >
      <Icon name="Send" size={14} />{$t("Submit Week")}
    </button>
    {#if !drafts.length && weekStatus !== "draft"}
      {#if pendingReopen}
        <div class="time-reopen-action">
          <span
            class="kz-chip kz-chip-pending"
            title={$t("Reopen pending approval.")}
          >
            {$t("Reopen pending approval.")}
          </span>
        </div>
      {:else if canRequestReopen}
        <div class="time-reopen-action">
          <button
            class="kz-btn kz-btn-sm"
            on:click={requestReopen}
            title={$t("Request edit")}
          >
            <Icon name="Edit" size={13} />{$t("Request edit")}
          </button>
        </div>
      {/if}
    {/if}
  </div>
</div>

<div class="content-area">
  {#if mo}
    <!-- Summary strip -->
    <div class="stat-cards">
      <div class="kz-card stat-card">
        <div class="stat-card-label">{$t("Logged")}</div>
        <div class="stat-card-value accent tab-num">
          {formatHours(weekHours)}
        </div>
        <div class="stat-card-sub">
          {$t("of {target} target", { target: targetHours })}
        </div>
      </div>
      <div class="kz-card stat-card">
        <div class="stat-card-label">{$t("Overtime")}</div>
        <div class="stat-card-value tab-num">{formatHours(overtime)}</div>
        <div class="stat-card-sub">{$t("this week")}</div>
      </div>
      <div class="kz-card stat-card">
        <div class="stat-card-label">{$t("Remaining")}</div>
        <div class="stat-card-value tab-num">{formatHours(remaining)}</div>
        <div class="stat-card-sub">{$t("to target")}</div>
      </div>
      <div class="kz-card stat-card">
        <div class="stat-card-label">{$t("Status")}</div>
        <div class="stat-card-value" style="font-size:16px">
          <span class="kz-chip kz-chip-{weekStatus}"
            >{statusLabel(weekStatus)}</span
          >
        </div>
      </div>
    </div>

    <!-- Week grid -->
    <div class="week-grid">
      {#each weekdays as day (day.ds)}
        {@const total = day.items.reduce(
          (s, e) =>
            e.status === "rejected"
              ? s
              : s + durMin(e.start_time.slice(0, 5), e.end_time.slice(0, 5)),
          0,
        )}
        {@const totalH = (total / 60).toFixed(1)}
        {@const dailyTarget = ($currentUser.weekly_hours || 0) / 5}
        <div
          class="kz-card day-card"
          class:day-card--locked={weekStatus === "submitted" ||
            weekStatus === "approved"}
          class:day-card--absent={day.absent}
        >
          <div class="day-header">
            <div>
              <div class="day-name">{$t(day.dayName)}</div>
              <div class="day-date tab-num">{fmtDateShort(day.d)}</div>
            </div>
            <div
              class="day-total tab-num"
              style="color: {total / 60 >= dailyTarget
                ? 'var(--accent)'
                : 'var(--text-primary)'}"
            >
              {formatHours(totalH)}
            </div>
          </div>

          <div class="day-entries">
            {#each day.items as e}
              {@const c = catOf(e.category_id, $categories)}
              <div
                class="time-block"
                on:click={() => {
                  if (e.status === "draft") showEntry = e;
                  else if (e.status === "submitted" || e.status === "approved")
                    showChange = e;
                }}
                on:keydown={() => {}}
                role="button"
                tabindex="0"
              >
                <div class="time-block-cat">
                  <span class="cat-dot" style="background:{c.color}"></span>
                  <span class="time-block-cat-name">{$t(c.name)}</span>
                  {#if e.status !== "draft"}
                    <span
                      class="kz-chip kz-chip-{e.status}"
                      style="height:18px;font-size:10px"
                    >
                      {statusLabel(e.status)}
                    </span>
                  {/if}
                </div>
                <div class="time-block-times tab-num">
                  <span>{entryTimeRange(e, timeFormat)}</span>
                  <span
                    >{formatHours(
                      durHours(
                        e.start_time.slice(0, 5),
                        e.end_time.slice(0, 5),
                      ),
                    )}</span
                  >
                </div>
              </div>
            {/each}
          </div>

          {#if (weekStatus === "draft" || drafts.length > 0) && day.ds <= today}
            <div class="day-add-btn">
              <button
                class="kz-btn kz-btn-ghost kz-btn-sm"
                style="width:100%;justify-content:center;border-style:dashed;border-color:var(--border)"
                disabled={day.absent || ($currentUser?.start_date && day.ds < $currentUser.start_date)}
                on:click={() => (showEntry = { entry_date: day.ds })}
              >
                <Icon name="Plus" size={13} />{$t("Add")}
              </button>
            </div>
          {/if}
        </div>
      {/each}
    </div>

    <!-- Weekend (Sat/Sun) if entries exist -->
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
            {#each day.items as e}
              {@const c = catOf(e.category_id, $categories)}
              <div class="time-block">
                <div class="time-block-cat">
                  <span class="cat-dot" style="background:{c.color}"></span>
                  <span class="time-block-cat-name">{$t(c.name)}</span>
                </div>
                <div class="time-block-times tab-num">
                  <span>{entryTimeRange(e, timeFormat)}</span>
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
      loadWeek(mo || new Date());
    }}
  />
{/if}
{#if showChange}
  <ChangeRequestDialog
    entry={showChange}
    onClose={() => {
      showChange = null;
      loadWeek(mo || new Date());
    }}
  />
{/if}
