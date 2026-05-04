<script>
  import { onDestroy, onMount } from "svelte";
  import flatpickr from "flatpickr";
  import "flatpickr/dist/flatpickr.min.css";
  import Icon from "./Icons.svelte";
  import { t } from "./i18n.js";
  import { settings } from "./stores.js";

  export let value = "";
  export let min = "";
  export let max = "";
  export let id = "";
  export let style = "";
  export let container = null;
  export let required = false;
  let cls = "kz-input tab-num";
  export { cls as class };

  let el;
  let fp;
  let lastContainer = container;
  let lastTimeFormat;

  $: timeFormat = $settings.time_format === "12h" ? "12h" : "24h";

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

  function syncAltInput() {
    const input = fp?.altInput;
    if (!input) return;
    if (id) input.id = id;
    if (style) input.setAttribute("style", style);
    else input.removeAttribute("style");
    input.readOnly = true;
    input.required = required;
    input.setAttribute("inputmode", "none");
  }

  function build(format) {
    if (fp) {
      removeAltInputListeners();
      fp.destroy();
    }
    lastContainer = container;
    lastTimeFormat = format;

    const opts = {
      enableTime: true,
      noCalendar: true,
      allowInput: false,
      clickOpens: false,
      disableMobile: true,
      dateFormat: "H:i",
      altInput: true,
      altInputClass: cls,
      altFormat: format === "12h" ? "h:i K" : "H:i",
      time_24hr: format !== "12h",
      defaultDate: value || null,
      minTime: min || null,
      maxTime: max || null,
      minuteIncrement: 1,
      onChange: (_, str) => {
        if (str !== value) value = str;
      },
    };

    if (container) {
      opts.appendTo = container;
      opts.static = true;
    }

    fp = flatpickr(el, opts);
    syncAltInput();
    if (fp.altInput) {
      fp.altInput.addEventListener("click", handleInputClick);
    }
  }

  onMount(() => build(timeFormat));
  onDestroy(() => {
    removeAltInputListeners();
    if (fp) fp.destroy();
  });

  $: if (fp && (timeFormat !== lastTimeFormat || container !== lastContainer)) {
    build(timeFormat);
  }
  $: if (fp && fp.input.value !== value) fp.setDate(value || null, false);
  $: if (fp) fp.set("minTime", min || null);
  $: if (fp) fp.set("maxTime", max || null);
  $: if (fp?.altInput) syncAltInput();
</script>

<span class="time-picker-wrap">
  <input bind:this={el} type="text" />
  <button
    type="button"
    class="time-picker-button"
    title={$t("Open time picker")}
    aria-label={$t("Open time picker")}
    on:click={openPicker}
  >
    <Icon name="Clock" size={14} />
  </button>
</span>

<style>
  .time-picker-wrap {
    position: relative;
    display: block;
    width: 100%;
  }

  .time-picker-wrap :global(.kz-input) {
    width: 100%;
    padding-right: 34px;
  }

  .time-picker-wrap :global(.flatpickr-wrapper) {
    display: block;
    width: 100%;
  }

  .time-picker-wrap :global(.flatpickr-calendar.static) {
    top: calc(100% + 6px);
    left: 0;
    margin-top: 0;
    box-shadow: var(--shadow-md);
  }

  .time-picker-button {
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

  .time-picker-button:hover,
  .time-picker-button:focus-visible {
    background: var(--bg-muted);
    color: var(--text-primary);
  }

  @media (max-width: 768px) {
    .time-picker-wrap :global(.flatpickr-calendar.static) {
      top: calc(100% + 8px);
    }
  }
</style>
