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
  .date-picker-wrap {
    position: relative;
    display: block;
    width: 100%;
  }

  .date-picker-wrap :global(.zf-input) {
    width: 100%;
    padding-right: 34px;
  }

  :global(.zf-date-picker-overlay) {
    box-shadow: var(--shadow-md);
    z-index: 999;
  }

  :global(.zf-date-picker-calendar:before),
  :global(.zf-date-picker-calendar:after) {
    left: var(--zf-date-picker-arrow-left, 22px);
    right: auto;
  }

  /* Month/year navigation header layout:
     [←][Month][→]  (left-aligned)          [Year] (right-aligned) */
  :global(.zf-date-picker-calendar .flatpickr-months) {
    position: relative;
    display: flex;
    align-items: center;
  }

  /* Pull arrows out of absolute positioning into the flex row */
  :global(.zf-date-picker-calendar .flatpickr-months .flatpickr-prev-month),
  :global(.zf-date-picker-calendar .flatpickr-months .flatpickr-next-month) {
    position: static;
    top: auto;
    height: auto;
    padding: 4px 6px;
  }

  /* Month container: shrink to content width only */
  :global(.zf-date-picker-calendar .flatpickr-months .flatpickr-month) {
    flex: 0 0 auto;
    position: static;
    height: auto;
    overflow: visible;
  }

  /* Current-month inner div: switch from absolute to inline-flex */
  :global(.zf-date-picker-calendar .flatpickr-current-month) {
    position: static;
    width: auto;
    left: auto;
    padding: 0;
    height: auto;
    font-size: 13.5px;
    display: inline-flex;
    align-items: center;
    text-align: left;
  }

  /* Year: absolutely positioned at the right edge of .flatpickr-months */
  :global(.zf-date-picker-calendar .flatpickr-current-month .numInputWrapper) {
    position: absolute;
    right: 8px;
    top: 50%;
    transform: translateY(-50%);
    width: 6ch;
    display: inline-flex;
    align-items: center;
  }

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
