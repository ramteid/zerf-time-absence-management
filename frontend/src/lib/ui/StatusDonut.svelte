<script>
  // Ring chart for a three-state breakdown (done / waiting / missing).
  // Each person contributes an equal share of the circle, so the coloured
  // arcs read directly as "how much of the team is finished".
  export let ready = 0;
  export let awaitingApproval = 0;
  export let notSubmitted = 0;
  export let size = 58;
  export let stroke = 26;

  // Geometry: the circle is drawn from the top (rotated -90deg in CSS) so the
  // first segment starts where a reader expects a progress ring to start.
  // The radius leaves room for half the stroke on each side (42 + 13 = 55),
  // so a thick ring still fits inside the 120-unit viewBox without clipping.
  const RADIUS = 42;
  const CIRCUMFERENCE = 2 * Math.PI * RADIUS;

  $: total = ready + awaitingApproval + notSubmitted;

  // Lay the arcs end to end by carrying a running offset. With no people at
  // all every arc is empty and only the grey track shows.
  $: segments = buildSegments(ready, awaitingApproval, notSubmitted);

  function buildSegments(readyCount, awaitingCount, missingCount) {
    const sum = readyCount + awaitingCount + missingCount;
    if (!sum) return [];
    let consumed = 0;
    return [
      { key: "ready", value: readyCount, className: "seg-ready" },
      { key: "awaiting", value: awaitingCount, className: "seg-awaiting" },
      { key: "missing", value: missingCount, className: "seg-missing" },
    ]
      .filter((segment) => segment.value > 0)
      .map((segment) => {
        const length = (segment.value / sum) * CIRCUMFERENCE;
        const offset = -(consumed / sum) * CIRCUMFERENCE;
        consumed += segment.value;
        return {
          ...segment,
          dash: `${length} ${CIRCUMFERENCE - length}`,
          offset,
        };
      });
  }
</script>

<svg
  class="status-donut"
  viewBox="0 0 120 120"
  width={size}
  height={size}
  role="img"
  aria-label={`${ready}/${total}`}
>
  <circle
    class="donut-track"
    cx="60"
    cy="60"
    r={RADIUS}
    stroke-width={stroke}
  />
  {#each segments as segment (segment.key)}
    <circle
      class="donut-segment {segment.className}"
      cx="60"
      cy="60"
      r={RADIUS}
      stroke-width={stroke}
      stroke-dasharray={segment.dash}
      stroke-dashoffset={segment.offset}
    />
  {/each}
</svg>

<style>
  .status-donut {
    /* Start the first arc at 12 o'clock instead of 3 o'clock. */
    transform: rotate(-90deg);
    flex-shrink: 0;
  }

  .donut-track,
  .donut-segment {
    fill: none;
  }

  .donut-track {
    stroke: var(--border);
  }

  .seg-ready {
    stroke: var(--success);
  }

  .seg-awaiting {
    stroke: var(--warning);
  }

  .seg-missing {
    stroke: var(--danger);
  }
</style>
