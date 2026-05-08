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

  // Index of the last data point up to and including yesterday.
  // The line and area fill stop here; x-axis labels cover the full range.
  $: todayStr = new Date().toISOString().slice(0, 10);
  $: lastActualIdx = (() => {
    let lastVisibleIndex = data.length - 1;
    while (lastVisibleIndex >= 0 && data[lastVisibleIndex].date > todayStr) lastVisibleIndex--;
    return lastVisibleIndex;
  })();

  // SVG layout constants
  const chartHeight = 230;
  const marginLeft = 54,
    marginRight = 16,
    marginTop = 16,
    marginBottom = 46;

  let containerW = 640;
  $: plotWidth = Math.max(1, containerW - marginLeft - marginRight);
  $: plotHeight = Math.max(1, chartHeight - marginTop - marginBottom);

  // Unique id to avoid clip-path collisions when multiple instances exist
  const chartInstanceId = Math.random().toString(36).slice(2, 8);

  // Value extents (always include 0)
  $: dataMin = data.reduce((minimumMinutes, day) => Math.min(minimumMinutes, day.cumulative_min), 0);
  $: dataMax = data.reduce((maximumMinutes, day) => Math.max(maximumMinutes, day.cumulative_min), 0);
  $: rawRange = Math.max(dataMax - dataMin, 60); // at least 1h

  // Display range with 10% padding
  $: dispMin = dataMin - rawRange * 0.1;
  $: dispMax = dataMax + rawRange * 0.1;
  $: dispRange = dispMax - dispMin;

  // Coordinate transforms
  $: xOf = (pointIndex) =>
    marginLeft + (data.length > 1 ? (pointIndex / (data.length - 1)) * plotWidth : plotWidth / 2);
  $: yOf = (minuteValue) => marginTop + plotHeight - ((minuteValue - dispMin) / dispRange) * plotHeight;
  $: zeroY = yOf(0);

  // Pre-compute point coordinates
  $: pts = data.map((day, pointIndex) => ({
    x: xOf(pointIndex),
    y: yOf(day.cumulative_min),
  }));

  // zeroY clamped to the plot area for clip-path rects.
  // Without clamping, when all values are same-sign the zero line falls outside
  // the plot and the area fill bleeds through the x-axis into the label area.
  $: clampedZeroY = Math.min(marginTop + plotHeight, Math.max(marginTop, zeroY));

  // Points subset: only up to today (for line and area)
  $: actualPts = lastActualIdx >= 0 ? pts.slice(0, lastActualIdx + 1) : [];

  // SVG path strings (only drawn up to today)
  $: linePath =
    actualPts.length < 2
      ? ""
      : actualPts
          .map(
            (point, pointIndex) =>
              `${pointIndex === 0 ? "M" : "L"}${point.x.toFixed(1)},${point.y.toFixed(1)}`,
          )
          .join(" ");

  $: areaPath =
    actualPts.length < 2
      ? ""
      : (() => {
          const firstPoint = actualPts[0];
          const lastPoint = actualPts[actualPts.length - 1];
          const innerPath = actualPts
            .map((point) => `${point.x.toFixed(1)},${point.y.toFixed(1)}`)
            .join(" L");
          const zeroLineY = zeroY.toFixed(1);
          return `M${firstPoint.x.toFixed(1)},${zeroLineY} L${innerPath} L${lastPoint.x.toFixed(1)},${zeroLineY} Z`;
        })();

  // Y-axis ticks
  $: yTicks = (() => {
    const rangeH = rawRange / 60;
    const step = rangeH / 5;
    const niceStepHours = [0.25, 0.5, 1, 2, 4, 8, 12, 24].find((stepHours) => stepHours >= step) || 24;
    const niceStepMinutes = niceStepHours * 60;
    // Start at first multiple of niceStepMinutes >= dataMin
    const start = Math.ceil(dataMin / niceStepMinutes) * niceStepMinutes;
    const ticks = [];
    for (let tickMinutes = start; tickMinutes <= dataMax + niceStepMinutes * 0.01; tickMinutes += niceStepMinutes) {
      ticks.push(tickMinutes);
    }
    if (!ticks.includes(0)) ticks.push(0);
    return ticks.sort((a, b) => a - b);
  })();

  // X-axis tick indices (~8 labels)
  $: xTickStep = Math.max(1, Math.ceil(data.length / 8));
  $: xTicks = data.reduce((tickIndexes, _day, pointIndex) => {
    if (pointIndex % xTickStep === 0) tickIndexes.push(pointIndex);
    return tickIndexes;
  }, /** @type {number[]} */ ([]));

  // Bar pixel width (for absence/holiday bands)
  $: barWidth = data.length > 1 ? plotWidth / (data.length - 1) : plotWidth;

  // ── Hover state ──────────────────────────────────────────────────────────
  let hoverIdx = /** @type {number|null} */ (null);

  function onMouseMove(e) {
    if (!data.length) return;
    const rect = e.currentTarget.getBoundingClientRect();
    const svgX = e.clientX - rect.left;
    const plotX = svgX - marginLeft;
    if (plotX < 0 || plotX > plotWidth) {
      hoverIdx = null;
      return;
    }
    const rawIndex = data.length > 1 ? (plotX / plotWidth) * (data.length - 1) : 0;
    const hoverIndex = Math.round(Math.max(0, Math.min(data.length - 1, rawIndex)));
    hoverIdx = hoverIndex <= lastActualIdx ? hoverIndex : null;
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
    const plotX = svgX - marginLeft;
    if (plotX < 0 || plotX > plotWidth) {
      hoverIdx = null;
      return;
    }
    const rawIndex = data.length > 1 ? (plotX / plotWidth) * (data.length - 1) : 0;
    const hoverIndex = Math.round(Math.max(0, Math.min(data.length - 1, rawIndex)));
    hoverIdx = hoverIndex <= lastActualIdx ? hoverIndex : null;
  }

  function onTouchEnd() {
    hoverIdx = null;
  }

  $: hoverD = hoverIdx !== null ? data[hoverIdx] : null;
  $: hoverPt = hoverIdx !== null ? pts[hoverIdx] : null;

  // ── Tooltip ──────────────────────────────────────────────────────────────
  const tooltipWidth = 172;
  const tooltipHeight = 70;

  $: tooltipX =
    hoverPt && hoverPt.x + tooltipWidth + marginRight + 10 > containerW
      ? hoverPt.x - tooltipWidth - 8
      : hoverPt
        ? hoverPt.x + 10
        : 0;
  $: tooltipY = hoverPt
    ? Math.max(marginTop, Math.min(chartHeight - marginBottom - tooltipHeight, hoverPt.y - tooltipHeight / 2))
    : 0;

  // ── Helpers ───────────────────────────────────────────────────────────────
  function fmtBal(min) {
    const isNegative = min < 0;
    const absoluteMinutes = Math.abs(min);
    const hours = Math.floor(absoluteMinutes / 60);
    const minutes = absoluteMinutes % 60;
    return formatHours(
      (isNegative ? "-" : "+") + hours + ":" + String(minutes).padStart(2, "0"),
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
    for (const day of data) {
      if (day.absence && !seen.has(day.absence)) {
        seen.add(day.absence);
        items.push({ key: day.absence, color: absColor(day.absence) });
      }
      if (day.holiday && !seen.has("__holiday__")) {
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
      height={chartHeight}
      style="display:block;overflow:visible"
      on:mousemove={onMouseMove}
      on:mouseleave={onMouseLeave}
      on:touchmove={onTouchMove}
      on:touchend={onTouchEnd}
    >
      <defs>
        <!-- clip above zero-line → positive area (green) -->
        <clipPath id="clip-above-{chartInstanceId}">
          <rect x={marginLeft} y={marginTop} width={plotWidth} height={clampedZeroY - marginTop} />
        </clipPath>
        <!-- clip below zero-line → negative area (red) -->
        <clipPath id="clip-below-{chartInstanceId}">
          <rect
            x={marginLeft}
            y={clampedZeroY}
            width={plotWidth}
            height={marginTop + plotHeight - clampedZeroY}
          />
        </clipPath>
      </defs>

      <!-- ── Absence / holiday vertical bands (only past/today) ── -->
      {#each data as day, pointIndex}
        {#if pointIndex <= lastActualIdx && (day.absence || day.holiday)}
          <rect
            x={pts[pointIndex].x - barWidth * 0.5}
            y={marginTop}
            width={barWidth}
            height={plotHeight}
            fill={day.absence ? absColor(day.absence) : "var(--warning)"}
            opacity="0.18"
          />
        {/if}
      {/each}

      <!-- ── Y-axis grid lines & labels ── -->
      {#each yTicks as tick}
        {@const tickY = yOf(tick)}
        <line
          x1={marginLeft}
          y1={tickY}
          x2={containerW - marginRight}
          y2={tickY}
          stroke={tick === 0 ? "var(--border-strong)" : "var(--border)"}
          stroke-width={tick === 0 ? 1.2 : 0.6}
          stroke-dasharray={tick === 0 ? undefined : "3 3"}
        />
        <text
          x={marginLeft - 5}
          y={tickY}
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
          clip-path="url(#clip-above-{chartInstanceId})"
        />
        <path
          d={areaPath}
          fill="var(--danger)"
          opacity="0.2"
          clip-path="url(#clip-below-{chartInstanceId})"
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
        x1={marginLeft}
        y1={chartHeight - marginBottom}
        x2={containerW - marginRight}
        y2={chartHeight - marginBottom}
        stroke="var(--border-strong)"
        stroke-width="1"
      />
      <line
        x1={marginLeft}
        y1={marginTop}
        x2={marginLeft}
        y2={chartHeight - marginBottom}
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
