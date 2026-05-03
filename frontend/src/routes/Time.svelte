<script>
  import { api } from "../api.js";
  import { categories, currentUser, path, go, toast } from "../stores.js";
  import { t, statusLabel, hoursUnit } from "../i18n.js";
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
    minToHM,
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
  let mo, su;
  let showEntry = null;
  let showChange = null;
  let myReopens = [];

  $: weekParam = (() => {
    const q = $path.includes("?") ? $path.split("?")[1] : "";
    return new URLSearchParams(q).get("week");
  })();

  async function load() {
    const date = weekParam ? parseDate(weekParam) : new Date();
    mo = monday(date);
    su = addDays(mo, 6);
    entries = await api(`/time-entries?from=${isoDate(mo)}&to=${isoDate(su)}`);
    try {
      myReopens = await api("/reopen-requests");
    } catch {
      myReopens = [];
    }
  }
  $: $path && load().catch(() => (entries = []));

  function gotoWeek(offset) {
    go("/time?week=" + isoDate(addDays(mo, offset)));
  }

  async function submitWeek(ids) {
    try {
      await api("/time-entries/submit", { method: "POST", body: { ids } });
      toast($t("Week submitted."), "ok");
      load();
    } catch (e) {
      toast(e.message || $t("Error"), "error");
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
      load();
    } catch (e) {
      toast(e.message || $t("Error"), "error");
    }
  }

  function catOf(id) {
    return $categories.find((c) => c.id === id) || { name: "?", color: "#999" };
  }

  $: hu = hoursUnit();

  $: weekActual = entries.reduce(
    (s, e) =>
      s +
      (e.start_time && e.end_time
        ? durMin(e.start_time.slice(0, 5), e.end_time.slice(0, 5))
        : 0),
    0,
  );
  $: weekTarget = Math.round(($currentUser.weekly_hours || 0) * 60);
  $: drafts = entries.filter((e) => e.status === "draft");
  $: weekHours = (weekActual / 60).toFixed(1);
  $: targetHours = ($currentUser.weekly_hours || 0).toFixed(1);
  $: overtime = Math.max(
    0,
    weekActual / 60 - ($currentUser.weekly_hours || 0),
  ).toFixed(1);
  $: remaining = Math.max(
    0,
    ($currentUser.weekly_hours || 0) - weekActual / 60,
  ).toFixed(1);

  function buildDay(i) {
    const d = addDays(mo, i);
    const ds = isoDate(d);
    return {
      d,
      ds,
      dayName: DAYS_FULL[i],
      items: entries
        .filter((e) => dateKey(e.entry_date) === ds)
        .sort((a, b) => a.start_time.localeCompare(b.start_time)),
    };
  }

  // Reactive day lists so Svelte re-renders when entries or mo change
  $: weekdays = mo ? [0, 1, 2, 3, 4].map(buildDay) : [];
  $: weekendDays = mo ? [5, 6].map(buildDay) : [];

  function durHours(start, end) {
    return (durMin(start, end) / 60).toFixed(1);
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

  $: currentWeekMo = monday(new Date());
  $: isCurrentWeek = mo && isoDate(mo) >= isoDate(currentWeekMo);

  $: weekStatus = (() => {
    if (entries.length === 0) return "draft";
    if (entries.every((e) => e.status === "approved")) return "approved";
    if (entries.some((e) => e.status === "submitted")) return "submitted";
    if (entries.some((e) => e.status === "rejected")) return "rejected";
    return "draft";
  })();

  $: pendingReopen = (() => {
    if (!mo) return null;
    const ws = isoDate(mo);
    return (
      myReopens.find((r) => r.week_start === ws && r.status === "pending") ||
      null
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
    {#if mo}
      <div class="top-bar-subtitle">
        {$t("Week {week}", { week: isoWeek(mo) })} · {$currentUser.weekly_hours}{hu}
        {$t("contract")}
      </div>
    {/if}
  </div>
  <div class="top-bar-actions">
    {#if mo}
      <div style="display:flex;align-items:center;gap:4px">
        <button
          class="kz-btn kz-btn-icon-sm kz-btn-ghost"
          on:click={() => gotoWeek(-7)}
        >
          <Icon name="ChevLeft" size={16} />
        </button>
        <span
          class="tab-num"
          style="font-size:13.5px;font-weight:500;min-width:140px;text-align:center"
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
    {#if drafts.length}
      <button
        class="kz-btn kz-btn-primary"
        on:click={() => submitWeek(drafts.map((x) => x.id))}
      >
        <Icon name="Send" size={14} />{$t("Submit Week")}
      </button>
    {:else if weekStatus !== "draft"}
      <span class="kz-chip kz-chip-{weekStatus}">{statusLabel(weekStatus)}</span
      >
      {#if pendingReopen}
        <span
          class="kz-chip kz-chip-pending"
          title={$t("Reopen pending approval.")}
        >
          {$t("Reopen pending approval.")}
        </span>
      {:else if canRequestReopen}
        <button
          class="kz-btn kz-btn-sm"
          on:click={requestReopen}
          title={$t("Request edit")}
        >
          <Icon name="Edit" size={13} />{$t("Request edit")}
        </button>
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
        <div class="stat-card-value accent tab-num">{weekHours}{hu}</div>
        <div class="stat-card-sub">
          {$t("of {target}h target", { target: targetHours })}
        </div>
      </div>
      <div class="kz-card stat-card">
        <div class="stat-card-label">{$t("Overtime")}</div>
        <div class="stat-card-value tab-num">{overtime}{hu}</div>
        <div class="stat-card-sub">{$t("this week")}</div>
      </div>
      <div class="kz-card stat-card">
        <div class="stat-card-label">{$t("Remaining")}</div>
        <div class="stat-card-value tab-num">{remaining}{hu}</div>
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
      {#each [0, 1, 2, 3, 4] as i}
        {@const day = buildDay(i)}
        {@const total = day.items.reduce(
          (s, e) =>
            s + durMin(e.start_time.slice(0, 5), e.end_time.slice(0, 5)),
          0,
        )}
        {@const totalH = (total / 60).toFixed(1)}
        {@const dailyTarget = ($currentUser.weekly_hours || 0) / 5}
        <div
          class="kz-card day-card"
          class:day-card--locked={weekStatus === "submitted" ||
            weekStatus === "approved"}
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
              {totalH}{hu}
            </div>
          </div>

          <div class="day-entries">
            {#each day.items as e}
              {@const c = catOf(e.category_id)}
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
                  <span
                    >{e.start_time.slice(0, 5)} – {e.end_time.slice(0, 5)}</span
                  >
                  <span
                    >{durHours(
                      e.start_time.slice(0, 5),
                      e.end_time.slice(0, 5),
                    )}{hu}</span
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
    {#each [5, 6] as i}
      {@const day = buildDay(i)}
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
              {@const c = catOf(e.category_id)}
              <div class="time-block">
                <div class="time-block-cat">
                  <span class="cat-dot" style="background:{c.color}"></span>
                  <span class="time-block-cat-name">{$t(c.name)}</span>
                </div>
                <div class="time-block-times tab-num">
                  <span
                    >{e.start_time.slice(0, 5)} – {e.end_time.slice(0, 5)}</span
                  >
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
      load();
    }}
  />
{/if}
{#if showChange}
  <ChangeRequestDialog
    entry={showChange}
    onClose={() => {
      showChange = null;
      load();
    }}
  />
{/if}
