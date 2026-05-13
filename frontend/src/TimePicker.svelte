<script>
  import { settings } from "./stores.js";
  import { t } from "./i18n.js";

  export let value = "";
  export let id = "";
  export let style = "";
  export let required = false;
  let inputClass = "zf-input tab-num";
  export { inputClass as class };

  $: use12h = $settings.time_format === "12h";

  function parseHHMM(timeValue) {
    if (!timeValue) return { hour24: 0, minuteValue: 0 };
    const parts = String(timeValue).split(":");
    return {
      hour24: Math.min(23, Math.max(0, parseInt(parts[0], 10) || 0)),
      minuteValue: Math.min(59, Math.max(0, parseInt(parts[1], 10) || 0)),
    };
  }

  function toHHMM(hour24, minuteValue) {
    return String(hour24).padStart(2, "0") + ":" + String(minuteValue).padStart(2, "0");
  }

  let hour24 = 0;
  let minuteValue = 0;
  let isPm = false;
  let isOpen = false;
  let anchorElement;
  let drumElement;

  $: {
    const parsedTime = parseHHMM(value);
    hour24 = parsedTime.hour24;
    minuteValue = parsedTime.minuteValue;
    isPm = hour24 >= 12;
  }

  $: displayHour = use12h ? (hour24 % 12 === 0 ? 12 : hour24 % 12) : hour24;
  $: hourMax = use12h ? 12 : 23;
  $: hourMin = use12h ? 1 : 0;

  $: displayLabel =
    String(displayHour).padStart(2, "0") +
    ":" +
    String(minuteValue).padStart(2, "0") +
    (use12h ? " " + (isPm ? "PM" : "AM") : "");

  function commit(nextHour24, nextMinuteValue) {
    const nextValue = toHHMM(nextHour24, nextMinuteValue);
    if (nextValue !== value) value = nextValue;
  }

  function stepHour(delta) {
    if (use12h) {
      const currentDisplayHour = displayHour;
      const nextDisplayHour = ((currentDisplayHour - 1 + delta + 12) % 12) + 1;
      const nextHour24 = isPm
        ? (nextDisplayHour === 12 ? 12 : nextDisplayHour + 12)
        : nextDisplayHour === 12
          ? 0
          : nextDisplayHour;
      commit(nextHour24, minuteValue);
    } else {
      commit(((hour24 + delta) % 24 + 24) % 24, minuteValue);
    }
  }

  const MIN_STEP = 15;

  function stepMinute(delta) {
    const step = delta * MIN_STEP;
    commit(hour24, ((minuteValue + step) % 60 + 60) % 60);
  }

  function toggleAmPm() {
    commit(Math.min(23, Math.max(0, isPm ? hour24 - 12 : hour24 + 12)), minuteValue);
  }

  // Wheel accumulator - step every 80px
  let wheelAccumulator = { hour: 0, minute: 0, meridiem: 0 };
  const WHEEL_STEP = 80;

  function onWheelH(e) {
    keyFocus = "hour";
    wheelAccumulator.hour += e.deltaY;
    while (wheelAccumulator.hour >= WHEEL_STEP) { stepHour(1); wheelAccumulator.hour -= WHEEL_STEP; }
    while (wheelAccumulator.hour <= -WHEEL_STEP) { stepHour(-1); wheelAccumulator.hour += WHEEL_STEP; }
  }
  function onWheelM(e) {
    keyFocus = "minute";
    wheelAccumulator.minute += e.deltaY;
    while (wheelAccumulator.minute >= WHEEL_STEP) { stepMinute(1); wheelAccumulator.minute -= WHEEL_STEP; }
    while (wheelAccumulator.minute <= -WHEEL_STEP) { stepMinute(-1); wheelAccumulator.minute += WHEEL_STEP; }
  }
  function onWheelAP(e) {
    keyFocus = "meridiem";
    wheelAccumulator.meridiem += e.deltaY;
    if (Math.abs(wheelAccumulator.meridiem) >= WHEEL_STEP) { toggleAmPm(); wheelAccumulator.meridiem = 0; }
  }

  // Touch drag per column
  let touchColumn = null;
  let touchLastY = null;
  let touchAccY = 0;
  const TOUCH_STEP = 32;

  function onTouchStartH(e) { keyFocus = "hour"; touchColumn = "hour"; touchLastY = e.touches[0].clientY; touchAccY = 0; }
  function onTouchStartM(e) { keyFocus = "minute"; touchColumn = "minute"; touchLastY = e.touches[0].clientY; touchAccY = 0; }
  function onTouchStartAP(e) { keyFocus = "meridiem"; touchColumn = "meridiem"; touchLastY = e.touches[0].clientY; touchAccY = 0; }

  function onTouchMove(e) {
    if (touchLastY === null) return;
    e.preventDefault();
    const currentTouchY = e.touches[0].clientY;
    touchAccY += touchLastY - currentTouchY;
    touchLastY = currentTouchY;
    while (touchAccY >= TOUCH_STEP) {
      if (touchColumn === "hour") stepHour(1);
      else if (touchColumn === "minute") stepMinute(1);
      else toggleAmPm();
      touchAccY -= TOUCH_STEP;
    }
    while (touchAccY <= -TOUCH_STEP) {
      if (touchColumn === "hour") stepHour(-1);
      else if (touchColumn === "minute") stepMinute(-1);
      else toggleAmPm();
      touchAccY += TOUCH_STEP;
    }
  }

  function onTouchEnd() { touchColumn = null; touchLastY = null; touchAccY = 0; }

  // Keyboard digit buffer
  let keyBuffer = "";
  let keyTimer = null;
  let keyFocus = "hour"; // "hour" | "minute" | "meridiem"

  function onKeyDown(e) {
    if (!isOpen) {
      if (e.key === "Enter" || e.key === " ") { e.preventDefault(); isOpen = true; }
      return;
    }
    const digit = e.key >= "0" && e.key <= "9" ? parseInt(e.key, 10) : null;
    if (e.key === "Escape") {
      e.preventDefault(); isOpen = false;
    } else if (e.key === "Enter") {
      e.preventDefault(); isOpen = false;
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      if (keyFocus === "hour") stepHour(-1);
      else if (keyFocus === "minute") stepMinute(-1);
      else toggleAmPm();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      if (keyFocus === "hour") stepHour(1);
      else if (keyFocus === "minute") stepMinute(1);
      else toggleAmPm();
    } else if (e.key === "ArrowRight" || e.key === "Tab") {
      e.preventDefault();
      keyFocus = keyFocus === "hour" ? "minute" : (keyFocus === "minute" && use12h ? "meridiem" : "hour");
    } else if (e.key === "ArrowLeft") {
      e.preventDefault();
      keyFocus = keyFocus === "meridiem" ? "minute" : keyFocus === "minute" ? "hour" : (use12h ? "meridiem" : "minute");
    } else if (e.key === "a" || e.key === "A") {
      if (use12h && isPm) toggleAmPm();
    } else if (e.key === "p" || e.key === "P") {
      if (use12h && !isPm) toggleAmPm();
    } else if (digit !== null) {
      e.preventDefault();
      clearTimeout(keyTimer);
      keyBuffer += String(digit);
      const typedNumber = parseInt(keyBuffer, 10);
      if (keyFocus === "hour") {
        if (typedNumber > hourMax || keyBuffer.length >= 2) {
          const clampedHour = Math.min(hourMax, Math.max(hourMin, typedNumber));
          if (use12h) commit(isPm ? (clampedHour === 12 ? 12 : clampedHour + 12) : clampedHour === 12 ? 0 : clampedHour, minuteValue);
          else commit(clampedHour, minuteValue);
          keyBuffer = ""; keyFocus = "minute";
        } else {
          if (use12h) commit(isPm ? (typedNumber === 12 ? 12 : typedNumber + 12) : typedNumber === 12 ? 0 : typedNumber, minuteValue);
          else commit(typedNumber, minuteValue);
          keyTimer = setTimeout(() => { keyBuffer = ""; keyFocus = "minute"; }, 1200);
        }
      } else if (keyFocus === "minute") {
        const snappedMinute = Math.round(Math.min(59, typedNumber) / MIN_STEP) * MIN_STEP % 60;
        if (typedNumber > 5 || keyBuffer.length >= 2) {
          commit(hour24, snappedMinute);
          keyBuffer = ""; keyFocus = use12h ? "meridiem" : "hour";
        } else {
          commit(hour24, snappedMinute);
          keyTimer = setTimeout(() => { keyBuffer = ""; keyFocus = use12h ? "meridiem" : "hour"; }, 1200);
        }
      }
    }
  }

  function openPicker() {
    isOpen = true;
    keyFocus = "hour";
    keyBuffer = "";
    wheelAccumulator = { hour: 0, minute: 0, meridiem: 0 };
    // Move keyboard focus to the drum so it receives keydown events.
    setTimeout(() => drumElement?.focus(), 0);
  }
  function closePicker() { isOpen = false; }

  function onClickOutside(e) {
    if (anchorElement && !anchorElement.contains(e.target)) closePicker();
  }

  // Items rendered in each drum column (prev2 prev1 SELECTED next1 next2)
  function hourItems(displayedHour, currentHour24, use12HourClock) {
    const items = [];
    for (let offset = -2; offset <= 2; offset++) {
      const displayValue = use12HourClock
        ? ((displayedHour - 1 + offset + 12) % 12) + 1
        : ((currentHour24 + offset) % 24 + 24) % 24;
      items.push({ offset, displayValue });
    }
    return items;
  }

  function minuteItems(selectedMinute) {
    return [-2, -1, 0, 1, 2].map((offset) => ({
      offset,
      displayValue: ((selectedMinute + offset * MIN_STEP) % 60 + 60) % 60,
    }));
  }

  $: hourColumnItems = hourItems(displayHour, hour24, use12h);
  $: minuteColumnItems = minuteItems(minuteValue);
</script>

<svelte:window on:click={onClickOutside} />

<input type="hidden" {id} {value} {required} />

<div class="tp-root" bind:this={anchorElement}>
  <button
    type="button"
    class="tp-display {inputClass}"
    {style}
    aria-label={$t("Time")}
    aria-expanded={isOpen}
    on:click={openPicker}
    on:keydown={onKeyDown}
  >{displayLabel}</button>

  {#if isOpen}
    <div
      class="tp-drum"
      role="dialog"
      aria-label={$t("Time")}
      bind:this={drumElement}
      tabindex="-1"
      on:click|stopPropagation
      on:keydown={onKeyDown}
    >
      <!-- Hour column -->
      <div
        class="tp-col"
        class:tp-col-active={keyFocus === "hour"}
        on:wheel|preventDefault={onWheelH}
        on:touchstart|nonpassive={onTouchStartH}
        on:touchmove|nonpassive={onTouchMove}
        on:touchend={onTouchEnd}
        role="group"
        aria-label={$t("Hours")}
      >
        {#each hourColumnItems as item (item.offset)}
          <button
            type="button"
            class="tp-item"
            class:tp-item-sel={item.offset === 0}
            tabindex="-1"
            on:click|stopPropagation={() => { for (let stepIndex = 0; stepIndex < Math.abs(item.offset); stepIndex++) stepHour(item.offset > 0 ? 1 : -1); keyFocus = "hour"; drumElement?.focus(); }}
          >{String(item.displayValue).padStart(2, "0")}</button>
        {/each}
      </div>

      <div class="tp-sep">:</div>

      <!-- Minute column -->
      <div
        class="tp-col"
        class:tp-col-active={keyFocus === "minute"}
        on:wheel|preventDefault={onWheelM}
        on:touchstart|nonpassive={onTouchStartM}
        on:touchmove|nonpassive={onTouchMove}
        on:touchend={onTouchEnd}
        role="group"
        aria-label={$t("Minutes")}
      >
        {#each minuteColumnItems as item (item.offset)}
          <button
            type="button"
            class="tp-item"
            class:tp-item-sel={item.offset === 0}
            tabindex="-1"
            on:click|stopPropagation={() => { for (let stepIndex = 0; stepIndex < Math.abs(item.offset); stepIndex++) stepMinute(item.offset > 0 ? 1 : -1); keyFocus = "minute"; drumElement?.focus(); }}
          >{String(item.displayValue).padStart(2, "0")}</button>
        {/each}
      </div>

      {#if use12h}
        <div
          class="tp-col tp-col-ampm"
          class:tp-col-active={keyFocus === "meridiem"}
          on:wheel|preventDefault={onWheelAP}
          on:touchstart|nonpassive={onTouchStartAP}
          on:touchmove|nonpassive={onTouchMove}
          on:touchend={onTouchEnd}
          role="group"
          aria-label={isPm ? "PM" : "AM"}
        >
          <button
            type="button"
            class="tp-item"
            class:tp-item-sel={!isPm}
            tabindex="-1"
            on:click|stopPropagation={() => { if (isPm) toggleAmPm(); drumElement?.focus(); }}
          >AM</button>
          <button
            type="button"
            class="tp-item"
            class:tp-item-sel={isPm}
            tabindex="-1"
            on:click|stopPropagation={() => { if (!isPm) toggleAmPm(); drumElement?.focus(); }}
          >PM</button>
        </div>
      {/if}
      <button
        type="button"
        class="tp-ok"
        on:click|stopPropagation={closePicker}
      >{$t("OK")}</button>
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
    font-weight: 400;
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

  .tp-ok {
    align-self: center;
    margin-left: 8px;
    padding: 4px 10px;
    font-size: 13.5px;
    font-weight: 400;
    font-family: inherit;
    color: var(--accent);
    background: transparent;
    border: 1px solid var(--accent);
    border-radius: var(--radius-md);
    cursor: pointer;
  }

  .tp-ok:hover {
    background: var(--accent);
    color: #fff;
  }

  .tp-drum:focus {
    outline: none;
  }
</style>
