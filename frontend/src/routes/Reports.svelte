<script>
  // Reports page for monthly and team-related statistics.
  // Delegates each section to a sub-component in routes/reports/.
  import { currentUser, toast } from "../stores.js";
  import { t } from "../i18n.js";
  import { tracksOwnTime } from "../rolePolicy.js";
  import { getUsersForReports } from "../lib/api/reportsApi.js";
  import EmployeeReport from "./reports/EmployeeReport.svelte";
  import TeamReport from "./reports/TeamReport.svelte";
  import CategoryReport from "./reports/CategoryReport.svelte";
  import AbsenceReport from "./reports/AbsenceReport.svelte";
  import TimesheetExport from "./reports/TimesheetExport.svelte";

  // Leads and admins load all users for the dropdowns. Non-lead roles only see
  // their own data.
  let users = [];
  let lastUsersLoadKey = "";
  let latestUsersLoadRequest = 0;

  function usersLoadKey(user) {
    return user?.id
      ? `${user.id}:${!!user?.permissions?.can_view_team_reports}:${user.tracks_time !== false}`
      : "";
  }

  function isCurrentUsersLoad(loadKey, requestId) {
    return loadKey === lastUsersLoadKey && requestId === latestUsersLoadRequest;
  }

  async function initUsers(loadKey, requestId, user) {
    try {
      const canTeam = !!user?.permissions?.can_view_team_reports;
      if (!user?.id) {
        if (isCurrentUsersLoad(loadKey, requestId)) {
          users = [];
        }
        return;
      }
      const loadedUsers = await getUsersForReports(canTeam, user);
      if (isCurrentUsersLoad(loadKey, requestId)) {
        users = loadedUsers;
      }
    } catch (e) {
      if (isCurrentUsersLoad(loadKey, requestId)) {
        toast($t(e?.message || "Error"), "error");
      }
    }
  }

  $: canViewTeamReports = !!$currentUser?.permissions?.can_view_team_reports;
  $: {
    const user = $currentUser;
    const loadKey = usersLoadKey(user);
    if (loadKey !== lastUsersLoadKey) {
      lastUsersLoadKey = loadKey;
      latestUsersLoadRequest += 1;
      initUsers(loadKey, latestUsersLoadRequest, user);
    }
  }
  // Pure-admin users (admins with tracks_time=false) have no personal data, so
  // the self-only sections (Category, Absence, Timesheet self-views) collapse
  // into team-style views as well. Also covers any other future case where the
  // logged-in user can view team reports but doesn't track their own time.
  $: currentUserTracksTime = tracksOwnTime($currentUser);
  $: isSelfOnlyReportsView = !canViewTeamReports && currentUserTracksTime;
</script>

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t("Reports")}</h1>
    <div class="top-bar-subtitle">
      {#if canViewTeamReports}
        {$t("Team hours overview")}
      {:else}
        {$t("Your hours overview")}
      {/if}
    </div>
  </div>
</div>

<div class="content-area">
  <EmployeeReport {users} {isSelfOnlyReportsView} />
  {#if canViewTeamReports}
    <TeamReport />
  {/if}
  <CategoryReport {isSelfOnlyReportsView} />
  <AbsenceReport {users} {isSelfOnlyReportsView} />
  <TimesheetExport {users} {isSelfOnlyReportsView} {canViewTeamReports} />
</div>
