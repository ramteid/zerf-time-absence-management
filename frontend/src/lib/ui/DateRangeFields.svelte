<script>
  import DatePicker from "../../DatePicker.svelte";
  import FormField from "./FormField.svelte";

  export let from = "";
  export let to = "";
  export let fromId = "from";
  export let toId = "to";
  export let fromLabel = "From";
  export let toLabel = "To";
  export let maxFrom = "";
  export let maxTo = "";
</script>

<div class="field-row">
  <FormField label={fromLabel} forId={fromId}>
    <DatePicker id={fromId} bind:value={from} max={maxFrom || to} />
  </FormField>
  <FormField label={toLabel} forId={toId}>
    <DatePicker id={toId} bind:value={to} min={from} max={maxTo} />
  </FormField>
</div>
