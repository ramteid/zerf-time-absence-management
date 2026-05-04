<script>
  export let value = "";
  export let min = "";
  export let max = "";
  export let id = "";
  export let style = "";
  export let required = false;
  let cls = "kz-input tab-num";
  export { cls as class };

  function handleChange(e) {
    value = e.target.value;
  }
</script>

<input
  type="time"
  class={cls}
  {id}
  {style}
  {required}
  value={value}
  min={min || undefined}
  max={max || undefined}
  on:change={handleChange}
/>
