<script>
  import { createEventDispatcher } from "svelte";
  import DayCard from "./DayCard.svelte";

  export let weekdays = [];
  export let currentUser = null;
  export let categories = [];
  export let weekStatus = "draft";
  export let drafts = [];
  export let timeFormat = "24h";
  export let today = "";
  export let isAssistant = false;

  const dispatch = createEventDispatcher();
</script>

<div class="week-grid">
  {#each weekdays as day, dayIndex (day.ds)}
    <DayCard
      {day}
      {dayIndex}
      {currentUser}
      {categories}
      {weekStatus}
      {drafts}
      {timeFormat}
      {today}
      {isAssistant}
      on:edit={(event) => dispatch("edit", event.detail)}
      on:add={(event) => dispatch("add", event.detail)}
    />
  {/each}
</div>
