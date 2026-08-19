<script>
  import DatePicker from "../../DatePicker.svelte";
  import FormField from "./FormField.svelte";

  export let from = "";
  export let to = "";
  export let fromId = "from";
  export let toId = "to";
  export let fromLabel = "From";
  export let toLabel = "To";
  export let minFrom = "";
  export let maxFrom = "";
  export let minTo = "";
  export let maxTo = "";

  function earlierIsoDate(first, second) {
    if (!first) return second || "";
    if (!second) return first;
    return first < second ? first : second;
  }

  function laterIsoDate(first, second) {
    if (!first) return second || "";
    if (!second) return first;
    return first > second ? first : second;
  }

  // A caller can provide global bounds while the two fields provide each
  // other with chronological bounds. Intersect them instead of letting one
  // silently override the other.
  $: fromMax = earlierIsoDate(maxFrom, to);
  $: toMin = laterIsoDate(minTo, from);
</script>

<div class="field-row">
  <FormField label={fromLabel} forId={fromId}>
    <DatePicker id={fromId} bind:value={from} min={minFrom} max={fromMax} />
  </FormField>
  <FormField label={toLabel} forId={toId}>
    <DatePicker id={toId} bind:value={to} min={toMin} max={maxTo} />
  </FormField>
</div>
