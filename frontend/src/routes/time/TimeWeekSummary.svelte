<script>
  import { t, statusLabel } from "../../i18n.js";
  import StatGrid from "../../lib/ui/StatGrid.svelte";
  import StatCard from "../../lib/ui/StatCard.svelte";
  import { weekStatusColor } from "../../lib/domain/time.js";

  export let entries = [];
  export let weekHasTarget = false;
  export let weekLoggedMinutes = 0;
  export let weekTargetMinutes = 0;
  export let weekLoggedHours = "";
  export let weekTargetHours = "";
  export let pendingReopen = null;
  export let status = "draft";
</script>

{#if entries.length > 0}
  <StatGrid>
    <StatCard
      label={$t("Logged")}
      value={weekLoggedHours}
      sub={weekHasTarget
        ? $t("of {target} target", { target: weekTargetHours })
        : ""}
      color={weekHasTarget
        ? weekLoggedMinutes >= weekTargetMinutes
          ? "var(--success-text)"
          : "var(--danger-text)"
        : "var(--text-primary)"}
    />

    <StatCard
      label={$t("Status")}
      color={pendingReopen ? "var(--warning-text)" : weekStatusColor(status)}
    >
      {pendingReopen
        ? $t("Waiting for release")
        : status === "submitted"
          ? $t("Waiting for approval")
          : statusLabel(status)}
    </StatCard>
  </StatGrid>
{/if}
