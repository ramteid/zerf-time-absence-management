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
  export let mobileNative = false;
  export let container = null;
  let cls = "zf-input";
  export { cls as class };

  let inputElement;
  let datePickerInstance;
  let lastLang;
  let lastMode = mode;
  let lastContainer = container;
  let cleanupNavHandlers = null;
  let cleanupPositionListeners = null;
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

  function removeCalendarNavHandlers() {
    if (!cleanupNavHandlers) return;
    cleanupNavHandlers();
    cleanupNavHandlers = null;
  }

  function removePositionListeners() {
    if (!cleanupPositionListeners) return;
    cleanupPositionListeners();
    cleanupPositionListeners = null;
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

  function visualViewportBounds() {
    const viewport = window.visualViewport;
    return {
      left: viewport?.offsetLeft ?? 0,
      top: viewport?.offsetTop ?? 0,
      width: viewport?.width ?? window.innerWidth,
      height: viewport?.height ?? window.innerHeight,
    };
  }

  // The calendar remains a dialog descendant so it stays in the same top layer,
  // but fixed positioning lets it escape the scrolling dialog body. Its size and
  // position are constrained to the currently visible viewport.
  function positionInDialog(instance, positionElement) {
    const calendar = instance.calendarContainer;
    const anchor = positionElement || instance.altInput || instance._input;
    if (!calendar || !anchor || !container) return;

    const anchorRect = anchor.getBoundingClientRect();
    const viewport = visualViewportBounds();
    const viewportRight = viewport.left + viewport.width;
    const viewportBottom = viewport.top + viewport.height;
    const availableWidth = Math.max(0, viewport.width - overlayMargin * 2);
    const availableHeight = Math.max(0, viewport.height - overlayMargin * 2);

    calendar.style.setProperty(
      "--zf-date-picker-max-width",
      `${Math.floor(availableWidth)}px`,
    );
    calendar.style.setProperty(
      "--zf-date-picker-max-height",
      `${Math.floor(availableHeight)}px`,
    );

    const calendarWidth = calendar.offsetWidth;
    const calendarHeight = Math.min(
      measureCalendarHeight(calendar),
      availableHeight,
    );
    const spaceBelow = viewportBottom - overlayMargin - anchorRect.bottom;
    const spaceAbove = anchorRect.top - viewport.top - overlayMargin;
    const showAbove =
      spaceBelow < calendarHeight + overlayGap && spaceAbove > spaceBelow;

    const minLeft = viewport.left + overlayMargin;
    const maxLeft = Math.max(
      minLeft,
      viewportRight - calendarWidth - overlayMargin,
    );
    const left = clamp(anchorRect.left, minLeft, maxLeft);
    const rawTop = showAbove
      ? anchorRect.top - calendarHeight - overlayGap
      : anchorRect.bottom + overlayGap;
    const minTop = viewport.top + overlayMargin;
    const maxTop = Math.max(
      minTop,
      viewportBottom - calendarHeight - overlayMargin,
    );
    const top = clamp(rawTop, minTop, maxTop);

    const arrowLeft = clamp(
      anchorRect.left - left + anchorRect.width / 2,
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
    calendar.style.position = "fixed";
    calendar.style.top = `${Math.round(top)}px`;
    calendar.style.left = `${Math.round(left)}px`;
    calendar.style.right = "auto";
    calendar.style.setProperty(
      "--zf-date-picker-arrow-left",
      `${Math.round(arrowLeft)}px`,
    );
  }

  function attachPositionListeners(instance) {
    removePositionListeners();
    if (!container || instance.isMobile) return;

    const updatePosition = () => {
      if (!instance.isOpen) return;
      const anchor = instance.altInput || instance._input;
      const scrollContainer = anchor?.closest(".dialog-body");
      if (anchor && scrollContainer) {
        const anchorRect = anchor.getBoundingClientRect();
        const scrollRect = scrollContainer.getBoundingClientRect();
        if (
          anchorRect.bottom <= scrollRect.top ||
          anchorRect.top >= scrollRect.bottom
        ) {
          instance.close();
          return;
        }
      }
      positionInDialog(instance, anchor);
    };
    const preventDialogEscape = (event) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      instance.close();
      (instance.altInput || instance._input)?.focus();
    };

    const viewport = window.visualViewport;
    window.addEventListener("resize", updatePosition);
    document.addEventListener("scroll", updatePosition, true);
    container.addEventListener("keydown", preventDialogEscape, true);
    viewport?.addEventListener("resize", updatePosition);
    viewport?.addEventListener("scroll", updatePosition);
    cleanupPositionListeners = () => {
      window.removeEventListener("resize", updatePosition);
      document.removeEventListener("scroll", updatePosition, true);
      container.removeEventListener("keydown", preventDialogEscape, true);
      viewport?.removeEventListener("resize", updatePosition);
      viewport?.removeEventListener("scroll", updatePosition);
    };
  }

  // For month pickers: disables the "next year" button when already at the max year.
  function updateNextYearBtnState(instance) {
    const cal = instance?.calendarContainer;
    if (!cal) return;
    const nextBtn = cal.querySelector(".flatpickr-next-month");
    if (!nextBtn) return;
    const maxDate = instance.config.maxDate;
    const maxYear = maxDate ? maxDate.getFullYear() : null;
    const atMax = maxYear !== null && instance.currentYear >= maxYear;
    nextBtn.classList.toggle("flatpickr-disabled", atMax);
  }

  // For month pickers: makes the year display read-only (navigation via ← → arrows only).
  function lockYearInput(instance) {
    const cal = instance?.calendarContainer;
    if (!cal) return;
    const yearInput = cal.querySelector("input.cur-year");
    if (yearInput) {
      yearInput.readOnly = true;
      yearInput.tabIndex = -1;
    }
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

  function bindCalendarNavHandlers(instance) {
    removeCalendarNavHandlers();
    if (mode === "month") return;
    const cal = instance.calendarContainer;
    const prevBtn = cal?.querySelector(".flatpickr-prev-month");
    const nextBtn = cal?.querySelector(".flatpickr-next-month");
    if (!prevBtn || !nextBtn) return;

    const handlePrev = (event) => {
      event.preventDefault();
      event.stopImmediatePropagation();
      if (!prevBtn.classList.contains("flatpickr-disabled")) {
        instance.changeMonth(-1);
      }
    };
    const handleNext = (event) => {
      event.preventDefault();
      event.stopImmediatePropagation();
      if (!nextBtn.classList.contains("flatpickr-disabled")) {
        instance.changeMonth(1);
      }
    };
    prevBtn.addEventListener("click", handlePrev, true);
    nextBtn.addEventListener("click", handleNext, true);
    cleanupNavHandlers = () => {
      prevBtn.removeEventListener("click", handlePrev, true);
      nextBtn.removeEventListener("click", handleNext, true);
    };
  }

  function build(lang) {
    if (datePickerInstance) {
      removePositionListeners();
      removeCalendarNavHandlers();
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
      // Let callers enable native mobile pickers for better fallback support.
      disableMobile: !mobileNative,
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
      onClose: removePositionListeners,
      onOpen: (_, __, instance) => {
        if (isMonth) updateNextYearBtnState(instance);
        attachPositionListeners(instance);
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
    // In month mode the arrows navigate by year, so we manage the next-year
    // button state ourselves. In date mode flatpickr handles it natively via
    // updateNavigationCurrentMonth (the arrows navigate month-by-month there).
    if (isMonth) {
      opts.onYearChange = (_, __, inst) => updateNextYearBtnState(inst);
    }
    // When rendered inside a <dialog>, append the calendar to the dialog so it
    // participates in the top-layer stacking context.
    if (container) {
      opts.appendTo = container;
      opts.position = positionInDialog;
    }
    datePickerInstance = flatpickr(inputElement, opts);
    if (value) datePickerInstance.setDate(value, false);
    datePickerInstance.calendarContainer?.classList.add(
      "zf-date-picker-calendar",
    );
    if (container)
      datePickerInstance.calendarContainer?.classList.add(
        "zf-date-picker-overlay",
      );
    rearrangeCalendarNav(datePickerInstance);
    bindCalendarNavHandlers(datePickerInstance);
    lockYearInput(datePickerInstance);
    if (isMonth) updateNextYearBtnState(datePickerInstance);
    if (datePickerInstance.isMobile && datePickerInstance.mobileInput) {
      for (const className of cls.split(/\s+/).filter(Boolean)) {
        datePickerInstance.mobileInput.classList.add(className);
      }
      datePickerInstance.mobileInput.tabIndex = 0;
      if (id) datePickerInstance.mobileInput.id = id;
    } else if (id && datePickerInstance.altInput) {
      datePickerInstance.altInput.id = id;
    }
    if (datePickerInstance.altInput) {
      // Keep native mobile keyboard closed while still allowing date selection.
      datePickerInstance.altInput.readOnly = true;
      datePickerInstance.altInput.setAttribute("inputmode", "none");
      datePickerInstance.altInput.addEventListener("click", handleInputClick);
    }
  }

  onMount(() => build($language));
  onDestroy(() => {
    removePositionListeners();
    removeCalendarNavHandlers();
    removeAltInputListeners();
    if (datePickerInstance) datePickerInstance.destroy();
  });

  // Rebuild on language/mode change
  $: if (
    datePickerInstance &&
    ($language !== lastLang || mode !== lastMode || container !== lastContainer)
  ) {
    // eslint-disable-next-line no-useless-assignment
    lastLang = $language;
    // eslint-disable-next-line no-useless-assignment
    lastMode = mode;
    // eslint-disable-next-line no-useless-assignment
    lastContainer = container;
    build($language);
  }
  // Reactive value/min/max sync
  $: if (datePickerInstance && datePickerInstance.input.value !== value)
    datePickerInstance.setDate(value || null, false);
  $: if (datePickerInstance) datePickerInstance.set("minDate", min || null);
  $: if (datePickerInstance) {
    datePickerInstance.set("maxDate", max || null);
    if (mode === "month") updateNextYearBtnState(datePickerInstance);
  }
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
    width: min(307.875px, var(--zf-date-picker-max-width, 307.875px));
    max-width: var(--zf-date-picker-max-width, calc(100vw - 16px));
    max-height: var(--zf-date-picker-max-height, calc(100vh - 16px));
    overflow-x: hidden;
    overflow-y: auto;
    overscroll-behavior: contain;
    background: var(--bg-surface);
    border: 1px solid var(--border);
    box-shadow: var(--shadow-md);
    color: var(--text-primary);
    border-radius: var(--radius-lg);
    font-family: var(--font-sans);
  }

  :global(.zf-date-picker-calendar .flatpickr-innerContainer),
  :global(.zf-date-picker-calendar .flatpickr-rContainer),
  :global(.zf-date-picker-calendar .flatpickr-days),
  :global(.zf-date-picker-calendar .dayContainer) {
    width: 100%;
    min-width: 0;
    max-width: 100%;
  }

  :global(.zf-date-picker-calendar .flatpickr-rContainer) {
    flex: 1;
  }

  :global(.zf-date-picker-calendar .flatpickr-day) {
    max-width: none;
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
  :global(.flatpickr-calendar.zf-date-picker-calendar.arrowTop:before) {
    border-bottom-color: var(--border);
  }
  :global(.flatpickr-calendar.zf-date-picker-calendar.arrowTop:after) {
    border-bottom-color: var(--bg-surface);
  }
  :global(.flatpickr-calendar.zf-date-picker-calendar.arrowBottom:before) {
    border-top-color: var(--border);
  }
  :global(.flatpickr-calendar.zf-date-picker-calendar.arrowBottom:after) {
    border-top-color: var(--bg-surface);
  }

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
    font-size: 0.875rem;
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
  :global(
    .flatpickr-calendar.zf-date-picker-calendar
      .flatpickr-prev-month.flatpickr-disabled
  ),
  :global(
    .flatpickr-calendar.zf-date-picker-calendar
      .flatpickr-next-month.flatpickr-disabled
  ) {
    display: flex;
    opacity: 0.3;
    pointer-events: none;
  }

  /* Month label (select dropdown or static span) */
  :global(.zf-date-picker-calendar .flatpickr-monthDropdown-months) {
    color: var(--text-primary);
    background: transparent;
    font-weight: 500;
    font-size: 0.875rem;
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

  /* Year input wrapper – pushed to the right by margin-left: auto.
     Year is display-only in all pickers; navigation uses ← → arrows. */
  :global(.zf-date-picker-calendar .flatpickr-current-month .numInputWrapper) {
    flex: 0 0 auto;
    margin-left: auto;
    width: 6ch;
    pointer-events: none;
  }
  :global(
    .zf-date-picker-calendar .flatpickr-current-month .numInputWrapper:hover
  ) {
    background: transparent;
  }
  :global(.zf-date-picker-calendar .flatpickr-current-month input.cur-year) {
    color: var(--text-primary);
    font-weight: 500;
  }
  /* Hide year spin arrows */
  :global(
    .zf-date-picker-calendar .flatpickr-current-month .numInputWrapper .arrowUp
  ),
  :global(
    .zf-date-picker-calendar
      .flatpickr-current-month
      .numInputWrapper
      .arrowDown
  ) {
    display: none !important;
  }

  /* ── Weekday header row ── */
  :global(.zf-date-picker-calendar .flatpickr-weekdays) {
    background: transparent;
    padding: 4px 0 2px;
  }
  :global(.zf-date-picker-calendar span.flatpickr-weekday) {
    background: transparent;
    color: var(--text-tertiary);
    font-size: 0.75rem;
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
    box-shadow:
      -5px 0 0 var(--accent-soft),
      5px 0 0 var(--accent-soft);
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
  :global(
    .zf-date-picker-calendar .flatpickr-monthSelect-month.flatpickr-disabled
  ) {
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
