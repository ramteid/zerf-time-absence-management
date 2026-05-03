<script>
  import { onMount, onDestroy, createEventDispatcher } from "svelte";
  import flatpickr from "flatpickr";
  import { German } from "flatpickr/dist/l10n/de.js";
  import "flatpickr/dist/flatpickr.min.css";
  import { language } from "./i18n.js";

  export let value = ""; // ISO yyyy-mm-dd, or yyyy-mm for month
  export let mode = "date"; // "date" | "month"
  export let min = "";
  export let max = "";
  export let disabled = false;
  export let placeholder = "";
  export let id = "";
  export let className = "kz-input";
  export let lang = ""; // optional override

  const dispatch = createEventDispatcher();

  let inputEl;
  let fp;
  let lastValue = value;

  function localeFor(l) {
    return l === "de" ? German : "default";
  }

  function buildOptions() {
    const isMonth = mode === "month";
    const opts = {
      locale: localeFor(lang || $language),
      allowInput: true,
      disableMobile: true,
      dateFormat: isMonth ? "Y-m" : "Y-m-d",
      altInput: true,
      altFormat: isMonth
        ? $language === "de"
          ? "F Y"
          : "F Y"
        : $language === "de"
          ? "d.m.Y"
          : "Y-m-d",
      onChange: (_, str) => {
        if (str !== value) {
          value = str;
          lastValue = str;
          dispatch("change", str);
        }
      },
      onClose: (_, str) => {
        if (str !== value) {
          value = str;
          lastValue = str;
          dispatch("change", str);
        }
      },
    };
    if (isMonth) {
      // Use the monthSelect plugin
      // Lazy-required below in onMount
    }
    if (min) opts.minDate = min;
    if (max) opts.maxDate = max;
    return opts;
  }

  async function init() {
    if (!inputEl) return;
    const opts = buildOptions();
    if (mode === "month") {
      const mod = await import("flatpickr/dist/plugins/monthSelect/index.js");
      await import("flatpickr/dist/plugins/monthSelect/style.css");
      opts.plugins = [
        mod.default({
          shorthand: false,
          dateFormat: "Y-m",
          altFormat: $language === "de" ? "F Y" : "F Y",
        }),
      ];
    }
    fp = flatpickr(inputEl, opts);
    if (value) fp.setDate(value, false);
  }

  onMount(() => {
    init();
  });

  onDestroy(() => {
    if (fp) fp.destroy();
  });

  // Reactively update min/max/value on the picker.
  $: if (fp && value !== lastValue) {
    lastValue = value;
    fp.setDate(value || null, false);
  }
  $: if (fp && min !== undefined) {
    fp.set("minDate", min || null);
  }
  $: if (fp && max !== undefined) {
    fp.set("maxDate", max || null);
  }

  // Re-create when language or mode changes
  let lastLang = $language;
  let lastMode = mode;
  $: if (fp && ($language !== lastLang || mode !== lastMode)) {
    lastLang = $language;
    lastMode = mode;
    fp.destroy();
    fp = null;
    init();
  }
</script>

<input
  bind:this={inputEl}
  {id}
  class={className}
  type="text"
  {placeholder}
  {disabled}
/>
