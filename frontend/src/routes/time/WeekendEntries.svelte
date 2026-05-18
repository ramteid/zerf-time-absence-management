<script>
  import { createEventDispatcher } from "svelte";
  import DayCard from "./DayCard.svelte";

  export let weekendDays = [];
  export let currentUser = null;
  export let categories = [];
  export let weekStatus = "draft";
  export let timeFormat = "24h";

  const dispatch = createEventDispatcher();
</script>

{#each weekendDays as day (day.ds)}
  {#if day.items.length > 0}
    <div style="margin-top:12px">
      <DayCard
        {day}
        dayIndex={6}
        {currentUser}
        {categories}
        {weekStatus}
        {timeFormat}
        weekend={true}
        on:edit={(event) => dispatch("edit", event.detail)}
      />
    </div>
  {/if}
{/each}
