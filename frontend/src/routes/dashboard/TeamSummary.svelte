<script>
  import { t } from "../../i18n.js";
  import StatCard from "../../lib/ui/StatCard.svelte";

  export let pendingWeeks = [];
  export let pendingAbsences = [];
  export let users = [];
</script>

<div class="dashboard-group">
  <div class="dashboard-group-label">{$t("My Team")}</div>
  <div class="stat-cards">
    <!-- Waiting for the approver's decision: amber, as on the Submissions
         card. Red there means the employee still has to act. -->
    <StatCard
      label={$t("Pending Weeks")}
      value={pendingWeeks.length}
      color={pendingWeeks.length > 0
        ? "var(--warning-text)"
        : "var(--success-text)"}
    />
    <StatCard
      label={$t("Absence Requests")}
      value={pendingAbsences.length}
      color={pendingAbsences.length > 0
        ? "var(--warning-text)"
        : "var(--success-text)"}
    />
    <StatCard label={$t("Users")} value={users.length} />
  </div>
</div>
