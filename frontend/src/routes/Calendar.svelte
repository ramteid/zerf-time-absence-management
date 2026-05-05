<script>
  import { tick } from "svelte";
  import { api } from "../api.js";
  import { path, go, currentUser, categories } from "../stores.js";
  import { t, absenceKindLabel } from "../i18n.js";
  import {
    fmtMonthYear,
    weekdayLabels,
    monday,
    addDays,
    isoDate,
    fmtDate,
    durMin,
    minToHM,
  } from "../format.js";
  import Icon from "../Icons.svelte";

  let entries = [];
  let holidays = [];
  let timeEntries = [];
  let year, month;
  let dlg;
  let popupCell = null;
  let loadSeq = 0;

  $: {
    const q = $path.includes("?") ? $path.split("?")[1] : "";
    const p = new URLSearchParams(q);
    const today = new Date();
    year = Number(p.get("year")) || today.getFullYear();
    month = Number(p.get("month")) || today.getMonth() + 1;
  }

  async function load() {
    const seq = ++loadSeq;
    const loadYear = year;
    const loadMonth = month;
    const ms = `${loadYear}-${String(loadMonth).padStart(2, "0")}`;
    const first = new Date(loadYear, loadMonth - 1, 1);
    const last = new Date(loadYear, loadMonth, 0);
    const [nextEntries, nextHolidays, nextTimeEntries, nextCategories] =
      await Promise.all([
        api(`/absences/calendar?month=${ms}`),
        api(`/holidays?year=${loadYear}`),
        api(`/time-entries?from=${isoDate(first)}&to=${isoDate(last)}`).catch(
          () => [],
        ),
        api("/categories").catch(() => $categories),
      ]);
    if (seq !== loadSeq) return;
    entries = nextEntries;
    holidays = nextHolidays;
    timeEntries = nextTimeEntries;
    categories.set(nextCategories);
  }
  $: year && month && load().catch(() => {});

  $: hMap = new Map(holidays.map((f) => [f.holiday_date, f.name]));

  // The calendar API is already team-scoped. Time entries remain personal,
  // so we still filter those defensively.
  $: myTimeEntries = timeEntries.filter((e) => e.user_id === $currentUser?.id);

  $: teMap = (() => {
    const map = new Map();
    for (const te of myTimeEntries) {
      const d =
        typeof te.entry_date === "string"
          ? te.entry_date.slice(0, 10)
          : isoDate(te.entry_date);
      if (!map.has(d)) map.set(d, []);
      map.get(d).push(te);
    }
    return map;
  })();

  $: categoryById = new Map(
    $categories.map((category) => [category.id, category]),
  );

  // Distinct, accessible palette. Red is reserved for error states only.
  const HOLIDAY_COLOR = "#f59e0b";
  const FALLBACK_COLORS = [
    "#2563eb",
    "#10b981",
    "#8b5cf6",
    "#14b8a6",
    "#ec4899",
    "#64748b",
    "#0f766e",
    "#7c3aed",
    "#0891b2",
    "#ca8a04",
    "#4f46e5",
    "#0d9488",
  ];
  const absColorMap = {
    vacation: "#f59e0b",
    sick: "#ef4444",
    training: "#3b82f6",
    special_leave: "#a855f7",
    unpaid: "#64748b",
    general_absence: "#6b7280",
    absent: "#9ca3af",
  };

  function absColor(kind) {
    return absColorMap[kind] || "#9ca3af";
  }

  function normalizeColor(color) {
    return /^#[0-9a-f]{6}$/i.test(color || "") ? color.toLowerCase() : null;
  }

  function fallbackColor(offset = 0, used = new Set()) {
    for (let i = 0; i < FALLBACK_COLORS.length; i++) {
      const color = FALLBACK_COLORS[(offset + i) % FALLBACK_COLORS.length];
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

  function rawCellEvents(c, entryMap, categoryMap, translate) {
    const evts = [];
    if (c.hol) {
      evts.push({
        key: "holiday",
        color: HOLIDAY_COLOR,
        label: translate("Holiday"),
        detail: c.hol,
      });
    }
    for (const a of c.absences) {
      const label = absenceKindLabel(a.kind);
      evts.push({
        key: `absence:${a.kind}`,
        color: absColor(a.kind),
        label,
        title: label,
        detail: absenceDetail(a),
      });
    }
    for (const e of entryMap.get(c.ds) || []) {
      const start = e.start_time?.slice(0, 5) || "";
      const end = e.end_time?.slice(0, 5) || "";
      const dur = start && end ? minToHM(durMin(start, end)) : "";
      const range = start && end ? `${start} - ${end}` : "";
      const detail = dur ? `${range} (${dur})` : range;
      evts.push({
        key: `work:${e.category_id ?? "unknown"}`,
        color: workBaseColor(e, evts.length, categoryMap),
        label: translate(workLabel(e, categoryMap)),
        detail,
      });
    }
    return evts;
  }

  function buildColorMap(baseCells, entryMap, categoryMap, translate) {
    const used = new Set();
    const assigned = new Map();
    for (const c of baseCells) {
      if (c.other) continue;
      for (const ev of rawCellEvents(c, entryMap, categoryMap, translate)) {
        if (assigned.has(ev.key)) continue;
        let color =
          normalizeColor(ev.color) || fallbackColor(assigned.size, used);
        if (used.has(color)) color = fallbackColor(assigned.size, used);
        assigned.set(ev.key, color);
        used.add(color);
      }
    }
    return assigned;
  }

  function cellEvents(c, entryMap, categoryMap, colorMap, translate) {
    return rawCellEvents(c, entryMap, categoryMap, translate).map((ev) => ({
      ...ev,
      color: colorMap.get(ev.key) || ev.color,
    }));
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

  const todayStr = isoDate(new Date());

  $: cells = (() => {
    const first = new Date(year, month - 1, 1);
    const start = monday(first);
    const out = [];
    for (let i = 0; i < 42; i++) {
      const d = addDays(start, i);
      const ds = isoDate(d);
      const other = d.getMonth() !== month - 1;
      const wd = (d.getDay() + 6) % 7;
      out.push({
        d,
        ds,
        other,
        weekend: wd >= 5,
        today: ds === todayStr,
        hol: hMap.get(ds),
        absences: entries.filter((e) => ds >= e.start_date && ds <= e.end_date),
      });
      if (i >= 34 && other && (i + 1) % 7 === 0) break;
    }
    return out;
  })();

  $: colorByKey = buildColorMap(cells, teMap, categoryById, $t);
  $: eventCells = cells.map((cell) => ({
    ...cell,
    events: cellEvents(cell, teMap, categoryById, colorByKey, $t),
  }));

  $: legendItems = (() => {
    const seen = new Map();
    for (const c of eventCells) {
      if (c.other) continue;
      for (const ev of c.events) {
        if (!seen.has(ev.key)) {
          seen.set(ev.key, { color: ev.color, label: ev.label });
        }
      }
    }
    return [...seen.values()];
  })();

  async function clickDay(c) {
    const evts = c.events;
    if (evts.length === 0) return;
    popupCell = { ...c, events: evts };
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
  <div class="top-bar-subtitle">
    {fmtMonthYear(new Date(year, month - 1, 1))}
  </div>
  <div class="top-bar-actions">
    <div style="display:flex;align-items:center;gap:4px">
      <button
        class="kz-btn kz-btn-icon-sm kz-btn-ghost"
        on:click={() => go("/calendar" + prev)}
      >
        <Icon name="ChevLeft" size={16} />
      </button>
      <span
        class="tab-num"
        style="font-size:13.5px;font-weight:500;min-width:140px;text-align:center"
      >
        {fmtMonthYear(new Date(year, month - 1, 1))}
      </span>
      <button
        class="kz-btn kz-btn-icon-sm kz-btn-ghost"
        on:click={() => go("/calendar" + next)}
      >
        <Icon name="ChevRight" size={16} />
      </button>
    </div>
  </div>
</div>

<div class="content-area">
  <div class="kz-card" style="padding:16px">
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
      <button class="kz-btn-icon-sm kz-btn-ghost" on:click={closeDlg}>
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
      <button class="kz-btn" on:click={closeDlg}>{$t("Close")}</button>
    </footer>
  {/if}
</dialog>
