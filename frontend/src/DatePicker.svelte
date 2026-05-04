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
  let cls = "kz-input";
  export { cls as class };

  let el;
  let fp;
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
    if (!fp) return;
    if (fp.isOpen) {
      fp.close();
      return;
    }
    fp.open();
  }

  function handleInputClick() {
    openPicker();
  }

  function removeAltInputListeners() {
    const input = fp?.altInput;
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
      "--kz-date-picker-arrow-left",
      `${Math.round(arrowLeft)}px`,
    );
  }

  function build(lang) {
    if (fp) {
      removeAltInputListeners();
      fp.destroy();
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
    fp = flatpickr(el, opts);
    fp.calendarContainer?.classList.add("kz-date-picker-calendar");
    if (container)
      fp.calendarContainer?.classList.add("kz-date-picker-overlay");
    if (id && fp.altInput) fp.altInput.id = id;
    if (fp.altInput) {
      if (style) fp.altInput.setAttribute("style", style);
      // Keep native mobile keyboard closed while still allowing date selection.
      fp.altInput.readOnly = true;
      fp.altInput.setAttribute("inputmode", "none");
      fp.altInput.addEventListener("click", handleInputClick);
    }
  }

  onMount(() => build($language));
  onDestroy(() => {
    removeAltInputListeners();
    if (fp) fp.destroy();
  });

  // Rebuild on language/mode change
  $: if (
    fp &&
    ($language !== lastLang || mode !== lastMode || container !== lastContainer)
  ) {
    lastLang = $language;
    lastMode = mode;
    lastContainer = container;
    build($language);
  }
  // Reactive value/min/max sync
  $: if (fp && fp.input.value !== value) fp.setDate(value || null, false);
  $: if (fp) fp.set("minDate", min || null);
  $: if (fp) fp.set("maxDate", max || null);
</script>

<span class="date-picker-wrap">
  <input bind:this={el} type="text" />
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

  .date-picker-wrap :global(.kz-input) {
    width: 100%;
    padding-right: 34px;
  }

  :global(.kz-date-picker-overlay) {
    box-shadow: var(--shadow-md);
    z-index: 999;
  }

  :global(.kz-date-picker-calendar:before),
  :global(.kz-date-picker-calendar:after) {
    left: var(--kz-date-picker-arrow-left, 22px);
    right: auto;
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
