<script>
  import { api } from "../api.js";
  import { categories, currentUser, path, go, toast } from "../stores.js";
  import { t, statusLabel } from "../i18n.js";
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

  $: weekParam = (() => {
    const q = $path.includes("?") ? $path.split("?")[1] : "";
    return new URLSearchParams(q).get("week");
  })();

  async function load() {
    const date = weekParam ? parseDate(weekParam) : new Date();
    mo = monday(date);
    su = addDays(mo, 6);
    entries = await api(`/time-entries?from=${isoDate(mo)}&to=${isoDate(su)}`);
  }
  $: $path && load().catch(() => (entries = []));

  function gotoWeek(offset) {
    go("/time?week=" + isoDate(addDays(mo, offset)));
  }

  async function copyLast() {
    const v = await api(
      `/time-entries?from=${isoDate(addDays(mo, -7))}&to=${isoDate(addDays(mo, -1))}`,
    );
    let n = 0;
    for (const e of v) {
      const d = isoDate(addDays(parseDate(e.entry_date), 7));
      if (parseDate(d) > new Date()) continue;
      try {
        await api("/time-entries", {
          method: "POST",
          body: {
            entry_date: d,
            start_time: e.start_time.slice(0, 5),
            end_time: e.end_time.slice(0, 5),
            category_id: e.category_id,
            comment: e.comment,
          },
        });
        n++;
      } catch {}
    }
    toast($t("Copied {count} entries.", { count: n }), "ok");
    load();
  }

  async function submitWeek(ids) {
    await api("/time-entries/submit", { method: "POST", body: { ids } });
    toast($t("Week submitted."), "ok");
    load();
  }

  function catOf(id) {
    return $categories.find((c) => c.id === id) || { name: "?", color: "#999" };
  }

  $: weekActual = entries.reduce(
    (s, e) => s + durMin(e.start_time.slice(0, 5), e.end_time.slice(0, 5)),
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

  function dayList(i) {
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

  function durHours(start, end) {
    return (durMin(start, end) / 60).toFixed(1);
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
</script>

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t("Time Entry")}</h1>
    {#if mo}
      <div class="top-bar-subtitle">
        {$t("Week {week}", { week: isoWeek(mo) })} · {$currentUser.weekly_hours}h
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
    <button
      class="kz-btn kz-btn-ghost kz-btn-sm"
      on:click={copyLast}
      disabled={weekStatus === "submitted" || weekStatus === "approved"}
    >
      {$t("Copy last week")}
    </button>
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
    {/if}
  </div>
</div>

<div class="content-area">
  {#if mo}
    <!-- Summary strip -->
    <div class="stat-cards">
      <div class="kz-card stat-card">
        <div class="stat-card-label">{$t("Logged")}</div>
        <div class="stat-card-value accent tab-num">{weekHours}h</div>
        <div class="stat-card-sub">
          {$t("of {target}h target", { target: targetHours })}
        </div>
      </div>
      <div class="kz-card stat-card">
        <div class="stat-card-label">{$t("Overtime")}</div>
        <div class="stat-card-value tab-num">{overtime}h</div>
        <div class="stat-card-sub">{$t("this week")}</div>
      </div>
      <div class="kz-card stat-card">
        <div class="stat-card-label">{$t("Remaining")}</div>
        <div class="stat-card-value tab-num">{remaining}h</div>
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
        {@const day = dayList(i)}
        {@const total = day.items.reduce(
          (s, e) =>
            s + durMin(e.start_time.slice(0, 5), e.end_time.slice(0, 5)),
          0,
        )}
        {@const totalH = (total / 60).toFixed(1)}
        {@const dailyTarget = ($currentUser.weekly_hours || 0) / 5}
        <div class="kz-card day-card">
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
              {totalH}h
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
                    )}h</span
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
      {@const day = dayList(i)}
      {#if day.items.length > 0}
        <div class="kz-card" style="margin-top:12px;overflow:hidden">
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
    onClose={(changed) => {
      showEntry = null;
      if (changed) load();
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
