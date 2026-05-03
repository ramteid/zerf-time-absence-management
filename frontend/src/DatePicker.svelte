<script>
  import { onMount, onDestroy } from "svelte";
  import flatpickr from "flatpickr";
  import { German } from "flatpickr/dist/l10n/de.js";
  import monthSelectPlugin from "flatpickr/dist/plugins/monthSelect/index.js";
  import "flatpickr/dist/flatpickr.min.css";
  import "flatpickr/dist/plugins/monthSelect/style.css";
  import { language } from "./i18n.js";

  export let value = "";
  export let mode = "date"; // "date" | "month"
  export let min = "";
  export let max = "";
  export let id = "";
  export let style = "";
  let cls = "kz-input";
  export { cls as class };

  let el;
  let fp;

  function build(lang) {
    if (fp) fp.destroy();
    const isMonth = mode === "month";
    const opts = {
      locale: lang === "de" ? German : "default",
      allowInput: true,
      disableMobile: true,
      dateFormat: isMonth ? "Y-m" : "Y-m-d",
      altInput: true,
      altInputClass: cls,
      altFormat: isMonth ? "F Y" : lang === "de" ? "d.m.Y" : "Y-m-d",
      defaultDate: value || null,
      minDate: min || null,
      maxDate: max || null,
      onChange: (_, str) => {
        if (str !== value) value = str;
      },
      plugins: isMonth
        ? [monthSelectPlugin({ shorthand: false, dateFormat: "Y-m", altFormat: "F Y" })]
        : [],
    };
    fp = flatpickr(el, opts);
    if (id && fp.altInput) fp.altInput.id = id;
    if (style && fp.altInput) fp.altInput.setAttribute("style", style);
  }

  onMount(() => build($language));
  onDestroy(() => fp && fp.destroy());

  // Rebuild on language/mode change
  let lastLang;
  let lastMode = mode;
  $: if (fp && ($language !== lastLang || mode !== lastMode)) {
    lastLang = $language;
    lastMode = mode;
    build($language);
  }
  // Reactive value/min/max sync
  $: if (fp && fp.input.value !== value) fp.setDate(value || null, false);
  $: if (fp) fp.set("minDate", min || null);
  $: if (fp) fp.set("maxDate", max || null);
</script>

<input bind:this={el} type="text" />
