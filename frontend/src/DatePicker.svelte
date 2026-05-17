<script>
  import { onMount, onDestroy } from "svelte";
  import flatpickr from "flatpickr";
  import { German } from "flatpickr/dist/l10n/de.js";
  import monthSelectPlugin from "flatpickr/dist/plugins/monthSelect/index.js";
  import "flatpickr/dist/flatpickr.min.css";
  import "flatpickr/dist/plugins/monthSelect/style.css";
  import Icon from "./Icons.svelte";
  import { language, t } from "./i18n.js";

  export let value = "";
  export let mode = "date"; // "date" | "month"
  export let min = "";
  export let max = "";
  export let id = "";
  export let style = "";
  export let container = null;
  let cls = "zf-input";
  export { cls as class };

  let inputElement;
  let datePickerInstance;
  let lastLang;
  let lastMode = mode;
  let lastContainer = container;
  const overlayGap = 6;
  const overlayMargin = 8;

  function validDate(year, monthIndex, day) {
    const parsed = new Date(year, monthIndex, day);
    if (
      parsed.getFullYear() !== year ||
      parsed.getMonth() !== monthIndex ||
      parsed.getDate() !== day
    ) {
      return undefined;
    }
    return parsed;
  }

  function parseInputDate(input) {
    const raw = String(input || "").trim();
    if (!raw) return undefined;
    if (mode === "month") {
      const isoMonth = raw.match(/^(\d{4})-(\d{1,2})$/);
      if (isoMonth) {
        return validDate(Number(isoMonth[1]), Number(isoMonth[2]) - 1, 1);
      }
      const localizedMonth = raw.match(/^(\d{1,2})\.(\d{4})$/);
      if (localizedMonth) {
        return validDate(
          Number(localizedMonth[2]),
          Number(localizedMonth[1]) - 1,
          1,
        );
      }
      return undefined;
    }

    const iso = raw.match(/^(\d{4})-(\d{1,2})-(\d{1,2})$/);
    if (iso) {
      return validDate(Number(iso[1]), Number(iso[2]) - 1, Number(iso[3]));
    }
    const localized = raw.match(/^(\d{1,2})\.(\d{1,2})\.(\d{4})$/);
    if (localized) {
      return validDate(
        Number(localized[3]),
        Number(localized[2]) - 1,
        Number(localized[1]),
      );
    }
    return undefined;
  }

  function openPicker() {
    if (!datePickerInstance) return;
    if (datePickerInstance.isOpen) {
      datePickerInstance.close();
      return;
    }
    datePickerInstance.open();
  }

  function handleInputClick() {
    openPicker();
  }

  function removeAltInputListeners() {
    const input = datePickerInstance?.altInput;
    if (!input) return;
    input.removeEventListener("click", handleInputClick);
  }

  function clamp(val, lo, hi) {
    return Math.min(Math.max(val, lo), hi);
  }

  function measureCalendarHeight(calendar) {
    const childHeight = Array.from(calendar.children).reduce(
      (total, child) => total + child.offsetHeight,
      0,
    );
    return childHeight || calendar.offsetHeight;
  }

  // Position the calendar so it floats above the dialog without affecting its
  // layout. The calendar is appended to the dialog (top-layer stacking context).
  // The dialog has `transform: translate(-50%, -50%)` which makes it the
  // containing block for position:fixed children, so we use position:absolute
  // with dialog-relative coordinates derived from getBoundingClientRect offsets.
  function positionInDialog(instance, positionElement) {
    const calendar = instance.calendarContainer;
    const anchor = positionElement || instance.altInput || instance._input;
    if (!calendar || !anchor || !container) return;

    const containerRect = container.getBoundingClientRect();
    const anchorRect = anchor.getBoundingClientRect();
    const calendarWidth = calendar.offsetWidth;
    const calendarHeight = measureCalendarHeight(calendar);
    const spaceBelow = window.innerHeight - anchorRect.bottom;
    const spaceAbove = anchorRect.top;
    const showAbove =
      spaceBelow < calendarHeight + overlayGap &&
      spaceAbove > calendarHeight + overlayGap;

    // Convert viewport-relative anchor coords to dialog-relative coords.
    const anchorLeft = anchorRect.left - containerRect.left;
    const maxLeft = Math.max(
      overlayMargin,
      containerRect.width - calendarWidth - overlayMargin,
    );
    const left = clamp(anchorLeft, overlayMargin, maxLeft);
    const top = showAbove
      ? anchorRect.top - containerRect.top - calendarHeight - overlayGap
      : anchorRect.bottom - containerRect.top + overlayGap;

    const arrowLeft = clamp(
      anchorLeft - left + anchorRect.width / 2,
      16,
      Math.max(16, calendarWidth - 16),
    );

    calendar.classList.remove(
      "arrowTop",
      "arrowBottom",
      "rightMost",
      "centerMost",
      "arrowLeft",
      "arrowCenter",
      "arrowRight",
    );
    calendar.classList.add(showAbove ? "arrowBottom" : "arrowTop");
    calendar.style.position = "absolute";
    calendar.style.top = `${Math.round(top)}px`;
    calendar.style.left = `${Math.round(left)}px`;
    calendar.style.right = "auto";
    calendar.style.setProperty(
      "--zf-date-picker-arrow-left",
      `${Math.round(arrowLeft)}px`,
    );
  }

  // Moves prev/next arrows inside .flatpickr-current-month so the DOM order
  // becomes [← Month → Year] instead of the flatpickr default [← Month Year →].
  function rearrangeCalendarNav(instance) {
    const cal = instance.calendarContainer;
    if (!cal) return;
    const months = cal.querySelector(".flatpickr-months");
    if (!months) return;
    const prevBtn = months.querySelector(".flatpickr-prev-month");
    const nextBtn = months.querySelector(".flatpickr-next-month");
    const currentMonthDiv = months.querySelector(".flatpickr-current-month");
    const numWrapper = currentMonthDiv?.querySelector(".numInputWrapper");
    if (!prevBtn || !nextBtn || !currentMonthDiv) return;
    currentMonthDiv.insertBefore(prevBtn, currentMonthDiv.firstChild);
    if (numWrapper) {
      currentMonthDiv.insertBefore(nextBtn, numWrapper);
    } else {
      currentMonthDiv.appendChild(nextBtn);
    }
  }

  function build(lang) {
    if (datePickerInstance) {
      removeAltInputListeners();
      datePickerInstance.destroy();
    }
    const isMonth = mode === "month";
    lastLang = lang;
    lastMode = mode;
    lastContainer = container;
    const opts = {
      locale: lang === "de" ? German : "default",
      allowInput: false,
      clickOpens: false,
      disableMobile: true,
      dateFormat: isMonth ? "Y-m" : "Y-m-d",
      altInput: true,
      altInputClass: cls,
      altFormat: isMonth ? "F Y" : lang === "de" ? "d.m.Y" : "Y-m-d",
      defaultDate: value || null,
      minDate: min || null,
      maxDate: max || null,
      parseDate: parseInputDate,
      onChange: (_, str) => {
        if (str !== value) value = str;
      },
      plugins: isMonth
        ? [
            monthSelectPlugin({
              shorthand: false,
              dateFormat: "Y-m",
              altFormat: "F Y",
            }),
          ]
        : [],
    };
    // When rendered inside a <dialog>, append the calendar to the dialog so it
    // participates in the top-layer stacking context. Use absolute positioning
    // with dialog-relative coordinates to avoid disrupting the dialog layout.
    if (container) {
      opts.appendTo = container;
      opts.position = positionInDialog;
    }
    datePickerInstance = flatpickr(inputElement, opts);
    if (value) datePickerInstance.setDate(value, false);
    datePickerInstance.calendarContainer?.classList.add("zf-date-picker-calendar");
    if (container)
      datePickerInstance.calendarContainer?.classList.add("zf-date-picker-overlay");
    rearrangeCalendarNav(datePickerInstance);
    if (id && datePickerInstance.altInput) datePickerInstance.altInput.id = id;
    if (datePickerInstance.altInput) {
      if (style) datePickerInstance.altInput.setAttribute("style", style);
      // Keep native mobile keyboard closed while still allowing date selection.
      datePickerInstance.altInput.readOnly = true;
      datePickerInstance.altInput.setAttribute("inputmode", "none");
      datePickerInstance.altInput.addEventListener("click", handleInputClick);
    }
  }

  onMount(() => build($language));
  onDestroy(() => {
    removeAltInputListeners();
    if (datePickerInstance) datePickerInstance.destroy();
  });

  // Rebuild on language/mode change
  $: if (
    datePickerInstance &&
    ($language !== lastLang || mode !== lastMode || container !== lastContainer)
  ) {
    lastLang = $language;
    lastMode = mode;
    lastContainer = container;
    build($language);
  }
  // Reactive value/min/max sync
  $: if (datePickerInstance && datePickerInstance.input.value !== value) datePickerInstance.setDate(value || null, false);
  $: if (datePickerInstance) datePickerInstance.set("minDate", min || null);
  $: if (datePickerInstance) datePickerInstance.set("maxDate", max || null);
</script>

<span class="date-picker-wrap">
  <input bind:this={inputElement} type="text" />
  <button
    type="button"
    class="date-picker-button"
    title={$t("Open calendar")}
    aria-label={$t("Open calendar")}
    on:click={openPicker}
  >
    <Icon name="Calendar" size={14} />
  </button>
</span>

<style>
  /* ── Input wrapper ── */
  .date-picker-wrap {
    position: relative;
    display: block;
    width: 100%;
  }
  .date-picker-wrap :global(.zf-input) {
    width: 100%;
    padding-right: 34px;
  }

  /* ── Calendar container base (light + dark theming) ── */
  :global(.flatpickr-calendar.zf-date-picker-calendar) {
    background: var(--bg-surface);
    border: 1px solid var(--border);
    box-shadow: var(--shadow-md);
    color: var(--text-primary);
    border-radius: var(--radius-lg);
    font-family: var(--font-sans);
  }

  /* Overlay z-index */
  :global(.zf-date-picker-overlay) {
    z-index: 999;
  }

  /* Tooltip arrow – positioning (left offset set from JS) */
  :global(.zf-date-picker-calendar:before),
  :global(.zf-date-picker-calendar:after) {
    left: var(--zf-date-picker-arrow-left, 22px);
    right: auto;
  }
  /* Tooltip arrow – colors match calendar surface (3 classes beats flatpickr's 2) */
  :global(.flatpickr-calendar.zf-date-picker-calendar.arrowTop:before) { border-bottom-color: var(--border); }
  :global(.flatpickr-calendar.zf-date-picker-calendar.arrowTop:after)  { border-bottom-color: var(--bg-surface); }
  :global(.flatpickr-calendar.zf-date-picker-calendar.arrowBottom:before) { border-top-color: var(--border); }
  :global(.flatpickr-calendar.zf-date-picker-calendar.arrowBottom:after)  { border-top-color: var(--bg-surface); }

  /* ── Month navigation header ── */
  /* After rearrangeCalendarNav() the DOM order inside .flatpickr-current-month is:
     [←]  [Month dropdown]  [→]  [Year]                                          */
  :global(.zf-date-picker-calendar .flatpickr-months) {
    display: flex;
    align-items: center;
    padding: 6px 10px;
    border-bottom: 1px solid var(--border);
  }
  :global(.zf-date-picker-calendar .flatpickr-months .flatpickr-month) {
    flex: 1;
    position: static;
    height: auto;
    overflow: visible;
    background: transparent;
    color: var(--text-primary);
    fill: var(--text-primary);
  }
  :global(.zf-date-picker-calendar .flatpickr-current-month) {
    position: static;
    width: 100%;
    left: auto;
    padding: 0;
    height: auto;
    font-size: 13px;
    font-weight: 500;
    display: flex;
    align-items: center;
    gap: 2px;
    text-align: left;
  }

  /* Prev / next arrows (moved inside .flatpickr-current-month by rearrangeCalendarNav) */
  :global(.zf-date-picker-calendar .flatpickr-prev-month),
  :global(.zf-date-picker-calendar .flatpickr-next-month) {
    position: static;
    top: auto;
    height: auto;
    padding: 4px;
    color: var(--text-tertiary);
    fill: var(--text-tertiary);
    border-radius: var(--radius-sm);
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  :global(.zf-date-picker-calendar .flatpickr-prev-month:hover),
  :global(.zf-date-picker-calendar .flatpickr-next-month:hover) {
    background: var(--bg-muted);
    color: var(--text-primary);
  }
  :global(.zf-date-picker-calendar .flatpickr-prev-month svg path),
  :global(.zf-date-picker-calendar .flatpickr-next-month svg path) {
    fill: currentColor;
  }
  /* Disabled arrows: always visible, just dimmed (4 classes beats flatpickr's 3) */
  :global(.flatpickr-calendar.zf-date-picker-calendar .flatpickr-prev-month.flatpickr-disabled),
  :global(.flatpickr-calendar.zf-date-picker-calendar .flatpickr-next-month.flatpickr-disabled) {
    display: flex;
    opacity: 0.3;
    pointer-events: none;
  }

  /* Month label (select dropdown or static span) */
  :global(.zf-date-picker-calendar .flatpickr-monthDropdown-months) {
    color: var(--text-primary);
    background: transparent;
    font-weight: 500;
    font-size: 13px;
    padding: 2px 4px;
    margin: 0;
    border-radius: var(--radius-sm);
  }
  :global(.zf-date-picker-calendar .flatpickr-monthDropdown-months:hover) {
    background: var(--bg-muted);
  }
  :global(.zf-date-picker-calendar .flatpickr-monthDropdown-months option) {
    background: var(--bg-surface);
    color: var(--text-primary);
  }
  :global(.zf-date-picker-calendar .cur-month) {
    color: var(--text-primary);
    font-weight: 500;
    margin-left: 0;
    padding: 2px 4px;
    border-radius: var(--radius-sm);
  }
  :global(.zf-date-picker-calendar .cur-month:hover) {
    background: var(--bg-muted);
  }

  /* Year input wrapper – pushed to the right by margin-left: auto */
  :global(.zf-date-picker-calendar .flatpickr-current-month .numInputWrapper) {
    flex: 0 0 auto;
    margin-left: auto;
    width: 6ch;
  }
  :global(.zf-date-picker-calendar .flatpickr-current-month .numInputWrapper:hover) {
    background: var(--bg-elevated);
    border-radius: var(--radius-sm);
  }
  :global(.zf-date-picker-calendar .flatpickr-current-month input.cur-year) {
    color: var(--text-primary);
    font-weight: 500;
  }

  /* ── Weekday header row ── */
  :global(.zf-date-picker-calendar .flatpickr-weekdays) {
    background: transparent;
    padding: 4px 0 2px;
  }
  :global(.zf-date-picker-calendar span.flatpickr-weekday) {
    background: transparent;
    color: var(--text-tertiary);
    font-size: 11px;
    font-weight: 600;
  }

  /* ── Day cells ── */
  :global(.zf-date-picker-calendar .flatpickr-day) {
    color: var(--text-primary);
    border-color: transparent;
    border-radius: var(--radius-sm);
  }
  :global(.zf-date-picker-calendar .flatpickr-day:hover),
  :global(.zf-date-picker-calendar .flatpickr-day:focus) {
    background: var(--bg-muted);
    border-color: var(--bg-muted);
    color: var(--text-primary);
  }
  :global(.zf-date-picker-calendar .flatpickr-day.today) {
    border-color: var(--accent);
  }
  :global(.zf-date-picker-calendar .flatpickr-day.today:hover),
  :global(.zf-date-picker-calendar .flatpickr-day.today:focus) {
    background: var(--accent);
    border-color: var(--accent);
    color: #fff;
  }
  :global(.zf-date-picker-calendar .flatpickr-day.selected),
  :global(.zf-date-picker-calendar .flatpickr-day.startRange),
  :global(.zf-date-picker-calendar .flatpickr-day.endRange),
  :global(.zf-date-picker-calendar .flatpickr-day.selected:hover),
  :global(.zf-date-picker-calendar .flatpickr-day.startRange:hover),
  :global(.zf-date-picker-calendar .flatpickr-day.endRange:hover) {
    background: var(--accent);
    border-color: var(--accent);
    color: #fff;
  }
  :global(.zf-date-picker-calendar .flatpickr-day.inRange) {
    background: var(--accent-soft);
    border-color: transparent;
    box-shadow: -5px 0 0 var(--accent-soft), 5px 0 0 var(--accent-soft);
    color: var(--accent-text);
  }
  :global(.zf-date-picker-calendar .flatpickr-day.prevMonthDay),
  :global(.zf-date-picker-calendar .flatpickr-day.nextMonthDay),
  :global(.zf-date-picker-calendar .flatpickr-day.notAllowed),
  :global(.zf-date-picker-calendar .flatpickr-day.flatpickr-disabled),
  :global(.zf-date-picker-calendar .flatpickr-day.flatpickr-disabled:hover) {
    color: var(--text-disabled);
    background: transparent;
    border-color: transparent;
  }

  /* ── Month-select plugin cells ── */
  :global(.zf-date-picker-calendar .flatpickr-monthSelect-month) {
    color: var(--text-primary);
    border-radius: var(--radius-sm);
  }
  :global(.zf-date-picker-calendar .flatpickr-monthSelect-month:hover),
  :global(.zf-date-picker-calendar .flatpickr-monthSelect-month:focus) {
    background: var(--bg-muted);
    color: var(--text-primary);
  }
  :global(.zf-date-picker-calendar .flatpickr-monthSelect-month.selected) {
    background: var(--accent);
    border-color: var(--accent);
    color: #fff;
  }
  :global(.zf-date-picker-calendar .flatpickr-monthSelect-month.flatpickr-disabled) {
    color: var(--text-disabled);
  }

  /* ── Open-calendar button ── */
  .date-picker-button {
    position: absolute;
    right: 4px;
    top: 50%;
    transform: translateY(-50%);
    width: 28px;
    height: 28px;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--text-tertiary);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
  }
  .date-picker-button:hover,
  .date-picker-button:focus-visible {
    background: var(--bg-muted);
    color: var(--text-primary);
  }
</style>
