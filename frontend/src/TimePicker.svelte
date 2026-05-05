<script>
  import { settings } from "./stores.js";
  import { t } from "./i18n.js";

  export let value = "";
  export let id = "";
  export let style = "";
  export let required = false;
  let cls = "kz-input tab-num";
  export { cls as class };

  $: use12h = $settings.time_format === "12h";

  function parseHHMM(v) {
    if (!v) return { h24: 0, mins: 0 };
    const parts = String(v).split(":");
    return {
      h24: Math.min(23, Math.max(0, parseInt(parts[0], 10) || 0)),
      mins: Math.min(59, Math.max(0, parseInt(parts[1], 10) || 0)),
    };
  }

  function toHHMM(h24, m) {
    return String(h24).padStart(2, "0") + ":" + String(m).padStart(2, "0");
  }

  let h24 = 0;
  let mins = 0;
  let pm = false;
  let open = false;
  let anchor;
  let drumEl;

  $: {
    const p = parseHHMM(value);
    h24 = p.h24;
    mins = p.mins;
    pm = h24 >= 12;
  }

  $: displayHour = use12h ? (h24 % 12 === 0 ? 12 : h24 % 12) : h24;
  $: hourMax = use12h ? 12 : 23;
  $: hourMin = use12h ? 1 : 0;

  $: displayLabel =
    String(displayHour).padStart(2, "0") +
    ":" +
    String(mins).padStart(2, "0") +
    (use12h ? " " + (pm ? "PM" : "AM") : "");

  function commit(newH24, newMins) {
    const c = toHHMM(newH24, newMins);
    if (c !== value) value = c;
  }

  function stepHour(delta) {
    if (use12h) {
      const cur = displayHour;
      const next = ((cur - 1 + delta + 12) % 12) + 1;
      const new24 = pm ? (next === 12 ? 12 : next + 12) : next === 12 ? 0 : next;
      commit(new24, mins);
    } else {
      commit(((h24 + delta) % 24 + 24) % 24, mins);
    }
  }

  function stepMinute(delta) {
    commit(h24, ((mins + delta) % 60 + 60) % 60);
  }

  function toggleAmPm() {
    commit(Math.min(23, Math.max(0, pm ? h24 - 12 : h24 + 12)), mins);
  }

  // Wheel accumulator - step every 80px
  let wheelAcc = { h: 0, m: 0, ap: 0 };
  const WHEEL_STEP = 80;

  function onWheelH(e) {
    keyFocus = "h";
    wheelAcc.h += e.deltaY;
    while (wheelAcc.h >= WHEEL_STEP) { stepHour(1); wheelAcc.h -= WHEEL_STEP; }
    while (wheelAcc.h <= -WHEEL_STEP) { stepHour(-1); wheelAcc.h += WHEEL_STEP; }
  }
  function onWheelM(e) {
    keyFocus = "m";
    wheelAcc.m += e.deltaY;
    while (wheelAcc.m >= WHEEL_STEP) { stepMinute(1); wheelAcc.m -= WHEEL_STEP; }
    while (wheelAcc.m <= -WHEEL_STEP) { stepMinute(-1); wheelAcc.m += WHEEL_STEP; }
  }
  function onWheelAP(e) {
    keyFocus = "ap";
    wheelAcc.ap += e.deltaY;
    if (Math.abs(wheelAcc.ap) >= WHEEL_STEP) { toggleAmPm(); wheelAcc.ap = 0; }
  }

  // Touch drag per column
  let touchCol = null;
  let touchLastY = null;
  let touchAccY = 0;
  const TOUCH_STEP = 32;

  function onTouchStartH(e) { keyFocus = "h"; touchCol = "h"; touchLastY = e.touches[0].clientY; touchAccY = 0; }
  function onTouchStartM(e) { keyFocus = "m"; touchCol = "m"; touchLastY = e.touches[0].clientY; touchAccY = 0; }
  function onTouchStartAP(e) { keyFocus = "ap"; touchCol = "ap"; touchLastY = e.touches[0].clientY; touchAccY = 0; }

  function onTouchMove(e) {
    if (touchLastY === null) return;
    e.preventDefault();
    const y = e.touches[0].clientY;
    touchAccY += touchLastY - y;
    touchLastY = y;
    while (touchAccY >= TOUCH_STEP) {
      if (touchCol === "h") stepHour(1);
      else if (touchCol === "m") stepMinute(1);
      else toggleAmPm();
      touchAccY -= TOUCH_STEP;
    }
    while (touchAccY <= -TOUCH_STEP) {
      if (touchCol === "h") stepHour(-1);
      else if (touchCol === "m") stepMinute(-1);
      else toggleAmPm();
      touchAccY += TOUCH_STEP;
    }
  }

  function onTouchEnd() { touchCol = null; touchLastY = null; touchAccY = 0; }

  // Keyboard digit buffer
  let keyBuf = "";
  let keyTimer = null;
  let keyFocus = "h"; // "h" | "m" | "ap"

  function onKeyDown(e) {
    if (!open) {
      if (e.key === "Enter" || e.key === " ") { e.preventDefault(); open = true; }
      return;
    }
    const digit = e.key >= "0" && e.key <= "9" ? parseInt(e.key, 10) : null;
    if (e.key === "Escape") {
      e.preventDefault(); open = false;
    } else if (e.key === "Enter") {
      e.preventDefault(); open = false;
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      if (keyFocus === "h") stepHour(-1);
      else if (keyFocus === "m") stepMinute(-1);
      else toggleAmPm();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      if (keyFocus === "h") stepHour(1);
      else if (keyFocus === "m") stepMinute(1);
      else toggleAmPm();
    } else if (e.key === "ArrowRight" || e.key === "Tab") {
      e.preventDefault();
      keyFocus = keyFocus === "h" ? "m" : (keyFocus === "m" && use12h ? "ap" : "h");
    } else if (e.key === "ArrowLeft") {
      e.preventDefault();
      keyFocus = keyFocus === "ap" ? "m" : keyFocus === "m" ? "h" : (use12h ? "ap" : "m");
    } else if (e.key === "a" || e.key === "A") {
      if (use12h && pm) toggleAmPm();
    } else if (e.key === "p" || e.key === "P") {
      if (use12h && !pm) toggleAmPm();
    } else if (digit !== null) {
      e.preventDefault();
      clearTimeout(keyTimer);
      keyBuf += String(digit);
      const num = parseInt(keyBuf, 10);
      if (keyFocus === "h") {
        if (num > hourMax || keyBuf.length >= 2) {
          const c = Math.min(hourMax, Math.max(hourMin, num));
          if (use12h) commit(pm ? (c === 12 ? 12 : c + 12) : c === 12 ? 0 : c, mins);
          else commit(c, mins);
          keyBuf = ""; keyFocus = "m";
        } else {
          if (use12h) commit(pm ? (num === 12 ? 12 : num + 12) : num === 12 ? 0 : num, mins);
          else commit(num, mins);
          keyTimer = setTimeout(() => { keyBuf = ""; keyFocus = "m"; }, 1200);
        }
      } else if (keyFocus === "m") {
        if (num > 5 || keyBuf.length >= 2) {
          commit(h24, Math.min(59, num));
          keyBuf = ""; keyFocus = use12h ? "ap" : "h";
        } else {
          commit(h24, num);
          keyTimer = setTimeout(() => { keyBuf = ""; keyFocus = use12h ? "ap" : "h"; }, 1200);
        }
      }
    }
  }

  function openPicker() {
    open = true;
    keyFocus = "h";
    keyBuf = "";
    wheelAcc = { h: 0, m: 0, ap: 0 };
    // Move keyboard focus to the drum so it receives keydown events.
    setTimeout(() => drumEl?.focus(), 0);
  }
  function closePicker() { open = false; }

  function onClickOutside(e) {
    if (anchor && !anchor.contains(e.target)) closePicker();
  }

  // Items rendered in each drum column (prev2 prev1 SELECTED next1 next2)
  function hourItems(dh, currentH24, u12h) {
    const items = [];
    for (let i = -2; i <= 2; i++) {
      const v = u12h
        ? ((dh - 1 + i + 12) % 12) + 1
        : ((currentH24 + i) % 24 + 24) % 24;
      items.push({ offset: i, v });
    }
    return items;
  }

  function minItems(m) {
    return [-2, -1, 0, 1, 2].map((i) => ({
      offset: i,
      v: ((m + i) % 60 + 60) % 60,
    }));
  }

  $: hItems = hourItems(displayHour, h24, use12h);
  $: mItems = minItems(mins);
</script>

<svelte:window on:click={onClickOutside} />

<input type="hidden" {id} {value} {required} />

<div class="tp-root" bind:this={anchor}>
  <button
    type="button"
    class="tp-display {cls}"
    {style}
    aria-label={$t("Time")}
    aria-expanded={open}
    on:click={openPicker}
    on:keydown={onKeyDown}
  >{displayLabel}</button>

  {#if open}
    <div
      class="tp-drum"
      role="dialog"
      aria-label={$t("Time")}
      bind:this={drumEl}
      tabindex="-1"
      on:click|stopPropagation
      on:keydown={onKeyDown}
    >
      <!-- Hour column -->
      <div
        class="tp-col"
        class:tp-col-active={keyFocus === "h"}
        on:wheel|preventDefault={onWheelH}
        on:touchstart|nonpassive={onTouchStartH}
        on:touchmove|nonpassive={onTouchMove}
        on:touchend={onTouchEnd}
        role="group"
        aria-label={$t("Hours")}
      >
        {#each hItems as item (item.offset)}
          <button
            type="button"
            class="tp-item"
            class:tp-item-sel={item.offset === 0}
            tabindex="-1"
            on:click|stopPropagation={() => { for (let i = 0; i < Math.abs(item.offset); i++) stepHour(item.offset > 0 ? 1 : -1); keyFocus = "h"; drumEl?.focus(); }}
          >{String(item.v).padStart(2, "0")}</button>
        {/each}
      </div>

      <div class="tp-sep">:</div>

      <!-- Minute column -->
      <div
        class="tp-col"
        class:tp-col-active={keyFocus === "m"}
        on:wheel|preventDefault={onWheelM}
        on:touchstart|nonpassive={onTouchStartM}
        on:touchmove|nonpassive={onTouchMove}
        on:touchend={onTouchEnd}
        role="group"
        aria-label={$t("Minutes")}
      >
        {#each mItems as item (item.offset)}
          <button
            type="button"
            class="tp-item"
            class:tp-item-sel={item.offset === 0}
            tabindex="-1"
            on:click|stopPropagation={() => { for (let i = 0; i < Math.abs(item.offset); i++) stepMinute(item.offset > 0 ? 1 : -1); keyFocus = "m"; drumEl?.focus(); }}
          >{String(item.v).padStart(2, "0")}</button>
        {/each}
      </div>

      {#if use12h}
        <div
          class="tp-col tp-col-ampm"
          class:tp-col-active={keyFocus === "ap"}
          on:wheel|preventDefault={onWheelAP}
          on:touchstart|nonpassive={onTouchStartAP}
          on:touchmove|nonpassive={onTouchMove}
          on:touchend={onTouchEnd}
          role="group"
          aria-label={pm ? "PM" : "AM"}
        >
          <button
            type="button"
            class="tp-item"
            class:tp-item-sel={!pm}
            tabindex="-1"
            on:click|stopPropagation={() => { if (pm) toggleAmPm(); drumEl?.focus(); }}
          >AM</button>
          <button
            type="button"
            class="tp-item"
            class:tp-item-sel={pm}
            tabindex="-1"
            on:click|stopPropagation={() => { if (!pm) toggleAmPm(); drumEl?.focus(); }}
          >PM</button>
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .tp-root {
    position: relative;
    width: 100%;
  }

  .tp-display {
    width: 100%;
    text-align: left;
    cursor: pointer;
    font-variant-numeric: tabular-nums;
  }

  .tp-drum {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    z-index: 999;
    display: flex;
    align-items: center;
    gap: 0;
    background: var(--bg-surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-md);
    padding: 6px 10px 10px;
    user-select: none;
    touch-action: none;
  }

  .tp-col {
    display: flex;
    flex-direction: column;
    align-items: center;
    cursor: pointer;
    border-radius: var(--radius-md);
    overflow: hidden;
    min-width: 36px;
  }

  .tp-col-active {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  .tp-col-ampm {
    min-width: 34px;
    margin-left: 4px;
    gap: 2px;
  }

  .tp-item {
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 13.5px;
    font-family: inherit;
    font-variant-numeric: tabular-nums;
    color: var(--text-tertiary);
    border-radius: var(--radius-sm);
    padding: 0 6px;
    transition: background 0.1s;
    width: 100%;
    background: transparent;
    border: none;
    cursor: pointer;
  }

  .tp-item:focus {
    outline: none;
  }

  .tp-item:hover {
    background: var(--bg-muted);
    color: var(--text-primary);
  }

  .tp-item-sel {
    color: var(--text-primary);
    font-weight: 600;
    font-size: 14.5px;
    background: var(--bg-muted);
  }

  .tp-sep {
    font-size: 15px;
    font-weight: 700;
    color: var(--text-secondary);
    padding: 0 3px;
    align-self: center;
  }

  .tp-drum:focus {
    outline: none;
  }
</style>
