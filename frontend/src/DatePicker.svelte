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
    // When rendered inside a <dialog>, append the calendar to the dialog
    // so it stays in the top layer and is not hidden behind the backdrop.
    if (container) {
      opts.appendTo = container;
      // Static mode keeps the calendar anchored to the input inside dialogs,
      // avoiding position offsets on small screens.
      opts.static = true;
    }
    fp = flatpickr(el, opts);
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

  .date-picker-wrap :global(.flatpickr-wrapper) {
    display: block;
    width: 100%;
  }

  .date-picker-wrap :global(.flatpickr-calendar.static) {
    top: calc(100% + 6px);
    left: 0;
    margin-top: 0;
    box-shadow: var(--shadow-md);
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

  @media (max-width: 768px) {
    .date-picker-wrap :global(.flatpickr-calendar.static) {
      top: calc(100% + 8px);
    }
  }
</style>
