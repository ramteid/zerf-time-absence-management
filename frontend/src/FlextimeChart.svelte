<script>
  import { t, absenceKindLabel, formatHours } from "./i18n.js";
  import { fmtDateShort } from "./format.js";

  /**
   * @typedef {{date: string, actual_min: number, target_min: number,
   *   diff_min: number, cumulative_min: number,
   *   absence: string|null, holiday: string|null}} FlextimeDay
   */

  /** @type {FlextimeDay[]} */
  export let data = [];

  // SVG layout constants
  const H = 230;
  const ML = 54,
    MR = 16,
    MT = 16,
    MB = 46;

  let containerW = 640;
  $: PW = Math.max(1, containerW - ML - MR);
  $: PH = Math.max(1, H - MT - MB);

  // Unique id to avoid clip-path collisions when multiple instances exist
  const uid = Math.random().toString(36).slice(2, 8);

  // Value extents (always include 0)
  $: dataMin = data.reduce((m, d) => Math.min(m, d.cumulative_min), 0);
  $: dataMax = data.reduce((m, d) => Math.max(m, d.cumulative_min), 0);
  $: rawRange = Math.max(dataMax - dataMin, 60); // at least 1h

  // Display range with 10% padding
  $: dispMin = dataMin - rawRange * 0.1;
  $: dispMax = dataMax + rawRange * 0.1;
  $: dispRange = dispMax - dispMin;

  // Coordinate transforms
  $: xOf = (i) =>
    ML + (data.length > 1 ? (i / (data.length - 1)) * PW : PW / 2);
  $: yOf = (v) => MT + PH - ((v - dispMin) / dispRange) * PH;
  $: zeroY = yOf(0);

  // Pre-compute point coordinates
  $: pts = data.map((d, i) => ({
    x: xOf(i),
    y: yOf(d.cumulative_min),
  }));

  // zeroY clamped to the plot area for clip-path rects.
  // Without clamping, when all values are same-sign the zero line falls outside
  // the plot and the area fill bleeds through the x-axis into the label area.
  $: clampedZeroY = Math.min(MT + PH, Math.max(MT, zeroY));

  // SVG path strings
  $: linePath =
    pts.length < 2
      ? ""
      : pts
          .map(
            (p, i) =>
              `${i === 0 ? "M" : "L"}${p.x.toFixed(1)},${p.y.toFixed(1)}`,
          )
          .join(" ");

  $: areaPath =
    pts.length < 2
      ? ""
      : (() => {
          const f = pts[0];
          const l = pts[pts.length - 1];
          const inner = pts
            .map((p) => `${p.x.toFixed(1)},${p.y.toFixed(1)}`)
            .join(" L");
          const z = zeroY.toFixed(1);
          return `M${f.x.toFixed(1)},${z} L${inner} L${l.x.toFixed(1)},${z} Z`;
        })();

  // Y-axis ticks
  $: yTicks = (() => {
    const rangeH = rawRange / 60;
    const step = rangeH / 5;
    const nice = [0.25, 0.5, 1, 2, 4, 8, 12, 24].find((s) => s >= step) || 24;
    const niceM = nice * 60;
    // Start at first multiple of niceM >= dataMin
    const start = Math.ceil(dataMin / niceM) * niceM;
    const ticks = [];
    for (let v = start; v <= dataMax + niceM * 0.01; v += niceM) {
      ticks.push(v);
    }
    if (!ticks.includes(0)) ticks.push(0);
    return ticks.sort((a, b) => a - b);
  })();

  // X-axis tick indices (~8 labels)
  $: xTickStep = Math.max(1, Math.ceil(data.length / 8));
  $: xTicks = data.reduce((acc, _d, i) => {
    if (i % xTickStep === 0) acc.push(i);
    return acc;
  }, /** @type {number[]} */ ([]));

  // Bar pixel width (for absence/holiday bands)
  $: barW = data.length > 1 ? PW / (data.length - 1) : PW;

  // ── Hover state ──────────────────────────────────────────────────────────
  let hoverIdx = /** @type {number|null} */ (null);

  function onMouseMove(e) {
    if (!data.length) return;
    const rect = e.currentTarget.getBoundingClientRect();
    const svgX = e.clientX - rect.left;
    const plotX = svgX - ML;
    if (plotX < 0 || plotX > PW) {
      hoverIdx = null;
      return;
    }
    const raw = data.length > 1 ? (plotX / PW) * (data.length - 1) : 0;
    hoverIdx = Math.round(Math.max(0, Math.min(data.length - 1, raw)));
  }

  function onMouseLeave() {
    hoverIdx = null;
  }

  function onTouchMove(e) {
    if (!data.length) return;
    e.preventDefault();
    const touch = e.touches[0];
    const rect = e.currentTarget.getBoundingClientRect();
    const svgX = touch.clientX - rect.left;
    const plotX = svgX - ML;
    if (plotX < 0 || plotX > PW) {
      hoverIdx = null;
      return;
    }
    const raw = data.length > 1 ? (plotX / PW) * (data.length - 1) : 0;
    hoverIdx = Math.round(Math.max(0, Math.min(data.length - 1, raw)));
  }

  function onTouchEnd() {
    hoverIdx = null;
  }

  $: hoverD = hoverIdx !== null ? data[hoverIdx] : null;
  $: hoverPt = hoverIdx !== null ? pts[hoverIdx] : null;

  // ── Tooltip ──────────────────────────────────────────────────────────────
  const TW = 172;
  const TH = 70;

  $: tooltipX =
    hoverPt && hoverPt.x + TW + MR + 10 > containerW
      ? hoverPt.x - TW - 8
      : hoverPt
        ? hoverPt.x + 10
        : 0;
  $: tooltipY = hoverPt
    ? Math.max(MT, Math.min(H - MB - TH, hoverPt.y - TH / 2))
    : 0;

  // ── Helpers ───────────────────────────────────────────────────────────────
  function fmtBal(min) {
    const neg = min < 0;
    const abs = Math.abs(min);
    const h = Math.floor(abs / 60);
    const m = abs % 60;
    return formatHours(
      (neg ? "-" : "+") + h + ":" + String(m).padStart(2, "0"),
    );
  }

  const ABSENCE_COLORS = {
    vacation: "#f59e0b",
    sick: "#ef4444",
    training: "#3b82f6",
    special_leave: "#a855f7",
    unpaid: "#64748b",
    general_absence: "#6b7280",
  };

  function absColor(kind) {
    return ABSENCE_COLORS[kind] || "var(--text-tertiary)";
  }

  // Legend: distinct absence/holiday kinds present in data
  $: legendItems = (() => {
    const seen = new Set();
    const items = [];
    for (const d of data) {
      if (d.absence && !seen.has(d.absence)) {
        seen.add(d.absence);
        items.push({ key: d.absence, color: absColor(d.absence) });
      }
      if (d.holiday && !seen.has("__holiday__")) {
        seen.add("__holiday__");
        items.push({ key: "__holiday__", color: "var(--warning)" });
      }
    }
    return items;
  })();
</script>

<!-- bind:clientWidth keeps PW in sync when the card resizes -->
<div
  bind:clientWidth={containerW}
  style="user-select:none;-webkit-user-select:none"
>
  {#if data.length === 0}
    <div
      style="text-align:center;padding:40px 0;color:var(--text-tertiary);font-size:13px"
    >
      {$t("No data.")}
    </div>
  {:else}
    <svg
      role="img"
      aria-label={$t("Flextime balance")}
      width={containerW}
      height={H}
      style="display:block;overflow:visible"
      on:mousemove={onMouseMove}
      on:mouseleave={onMouseLeave}
      on:touchmove={onTouchMove}
      on:touchend={onTouchEnd}
    >
      <defs>
        <!-- clip above zero-line → positive area (green) -->
        <clipPath id="clip-above-{uid}">
          <rect x={ML} y={MT} width={PW} height={clampedZeroY - MT} />
        </clipPath>
        <!-- clip below zero-line → negative area (red) -->
        <clipPath id="clip-below-{uid}">
          <rect
            x={ML}
            y={clampedZeroY}
            width={PW}
            height={MT + PH - clampedZeroY}
          />
        </clipPath>
      </defs>

      <!-- ── Absence / holiday vertical bands ── -->
      {#each data as d, i}
        {#if d.absence || d.holiday}
          <rect
            x={pts[i].x - barW * 0.5}
            y={MT}
            width={barW}
            height={PH}
            fill={d.absence ? absColor(d.absence) : "var(--warning)"}
            opacity="0.18"
          />
        {/if}
      {/each}

      <!-- ── Y-axis grid lines & labels ── -->
      {#each yTicks as tick}
        {@const ty = yOf(tick)}
        <line
          x1={ML}
          y1={ty}
          x2={containerW - MR}
          y2={ty}
          stroke={tick === 0 ? "var(--border-strong)" : "var(--border)"}
          stroke-width={tick === 0 ? 1.2 : 0.6}
          stroke-dasharray={tick === 0 ? undefined : "3 3"}
        />
        <text
          x={ML - 5}
          y={ty}
          text-anchor="end"
          dominant-baseline="middle"
          font-size="10"
          fill={tick === 0 ? "var(--text-secondary)" : "var(--text-tertiary)"}
        >
          {formatHours(
            (tick >= 0 ? "+" : "") +
              (tick / 60).toFixed(tick % 60 === 0 ? 0 : 1),
          )}
        </text>
      {/each}

      <!-- ── Area fill (green above / red below zero) ── -->
      {#if areaPath}
        <path
          d={areaPath}
          fill="var(--accent)"
          opacity="0.2"
          clip-path="url(#clip-above-{uid})"
        />
        <path
          d={areaPath}
          fill="var(--danger)"
          opacity="0.2"
          clip-path="url(#clip-below-{uid})"
        />
      {/if}

      <!-- ── Line ── -->
      {#if linePath}
        <path
          d={linePath}
          fill="none"
          stroke="var(--accent)"
          stroke-width="2"
          stroke-linejoin="round"
          stroke-linecap="round"
        />
      {/if}

      <!-- ── Axes ── -->
      <line
        x1={ML}
        y1={H - MB}
        x2={containerW - MR}
        y2={H - MB}
        stroke="var(--border-strong)"
        stroke-width="1"
      />
      <line
        x1={ML}
        y1={MT}
        x2={ML}
        y2={H - MB}
        stroke="var(--border)"
        stroke-width="1"
      />

      <!-- ── X-axis tick labels ── -->
      {#each xTicks as i}
        <text
          x={pts[i].x}
          y={H - MB + 14}
          text-anchor="middle"
          font-size="10"
          fill="var(--text-tertiary)"
        >
          {fmtDateShort(data[i].date)}
        </text>
      {/each}

      <!-- ── Crosshair + tooltip ── -->
      {#if hoverIdx !== null && hoverPt && hoverD}
        <!-- Vertical crosshair line -->
        <line
          x1={hoverPt.x}
          y1={MT}
          x2={hoverPt.x}
          y2={H - MB}
          stroke="var(--text-tertiary)"
          stroke-width="1"
          stroke-dasharray="4 3"
        />
        <!-- Horizontal crosshair line -->
        <line
          x1={ML}
          y1={hoverPt.y}
          x2={containerW - MR}
          y2={hoverPt.y}
          stroke="var(--text-tertiary)"
          stroke-width="1"
          stroke-dasharray="4 3"
        />
        <!-- Dot on the curve -->
        <circle
          cx={hoverPt.x}
          cy={hoverPt.y}
          r="4"
          fill="var(--accent)"
          stroke="white"
          stroke-width="1.5"
        />

        <!-- Tooltip card -->
        <rect
          x={tooltipX}
          y={tooltipY}
          width={TW}
          height={TH}
          rx="4"
          fill="var(--bg-surface)"
          stroke="var(--border-strong)"
          stroke-width="1"
          filter="drop-shadow(0 2px 6px rgba(0,0,0,0.1))"
        />
        <!-- Date (+ absence/holiday label) -->
        <text
          x={tooltipX + 9}
          y={tooltipY + 17}
          font-size="11"
          font-weight="600"
          fill="var(--text-primary)"
        >
          {fmtDateShort(hoverD.date)}{hoverD.absence
            ? " · " + absenceKindLabel(hoverD.absence)
            : hoverD.holiday
              ? " · " + hoverD.holiday
              : ""}
        </text>
        <!-- Balance -->
        <text
          x={tooltipX + 9}
          y={tooltipY + 34}
          font-size="11"
          fill="var(--text-secondary)"
        >
          {$t("Balance")}: {fmtBal(hoverD.cumulative_min)}
        </text>
        <!-- Daily diff -->
        <text
          x={tooltipX + 9}
          y={tooltipY + 51}
          font-size="11"
          fill="var(--text-tertiary)"
        >
          {$t("Daily diff")}: {fmtBal(hoverD.diff_min)}
        </text>
      {/if}
    </svg>

    <!-- ── Legend ── -->
    {#if legendItems.length}
      <div
        style="display:flex;gap:14px;flex-wrap:wrap;margin-top:4px;padding-left:{ML}px"
      >
        {#each legendItems as item}
          <div
            style="display:flex;align-items:center;gap:4px;font-size:11px;color:var(--text-secondary)"
          >
            <div
              style="width:10px;height:10px;border-radius:2px;background:{item.color};opacity:0.7;flex-shrink:0"
            ></div>
            {item.key === "__holiday__"
              ? $t("Holidays")
              : absenceKindLabel(item.key)}
          </div>
        {/each}
      </div>
    {/if}
  {/if}
</div>
