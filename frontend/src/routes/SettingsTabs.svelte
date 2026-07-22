<script>
  import { path, currentUser, go } from "../stores.js";
  import { t } from "../i18n.js";

  $: pathname = (() => {
    const queryIndex = $path.indexOf("?");
    return queryIndex >= 0 ? $path.slice(0, queryIndex) : $path;
  })();

  $: isAdmin = !!$currentUser?.permissions?.can_manage_settings;
  $: isLead = !!$currentUser?.permissions?.can_manage_team_settings;
  // Scoped "assistant" user management, granted to non-admin team leads only
  // (admins already have the full Users tab above).
  $: canManageTeamUsers = !!$currentUser?.permissions?.can_manage_team_users;

  // Admin-only tabs — visible only to admins.
  const adminTabs = [
    { href: "/settings/general", key: "Settings" },
    { href: "/settings/users", key: "Users" },
    { href: "/settings/categories", key: "Categories" },
    { href: "/settings/holidays", key: "Holidays" },
    { href: "/settings/email", key: "Email" },
    { href: "/settings/upload", key: "Nextcloud Backups" },
    { href: "/settings/audit-log", key: "Audit Log" },
    { href: "/settings/system-log", key: "System Log" },
  ];

  // The team-settings tab is shown to all leads (including admin leads).
  const teamTab = { href: "/settings/team", key: "Team Settings" };
  const teamUsersTab = { href: "/settings/team-users", key: "Users" };

  // For admins, Team Settings sits between the general "Settings" tab and
  // "Users" (i.e. after the first admin tab). Non-admin leads see it first.
  $: tabs = isAdmin
    ? [adminTabs[0], teamTab, ...adminTabs.slice(1)]
    : isLead
      ? canManageTeamUsers
        ? [teamTab, teamUsersTab]
        : [teamTab]
      : [];

  function onSelectChange(event) {
    const href = event.target.value;
    if (href) go(href);
  }
</script>

<!-- Desktop: horizontal tab bar -->
<div class="admin-tabs desktop-tabs">
  {#each tabs as tab (tab.href)}
    <a
      href={tab.href}
      data-link="1"
      class="tab-link"
      class:active={pathname === tab.href}
    >
      {$t(tab.key)}
    </a>
  {/each}
</div>

<!-- Mobile: styled select dropdown -->
<div class="mobile-tabs">
  <select on:change={onSelectChange}>
    {#each tabs as tab (tab.href)}
      <option value={tab.href} selected={pathname === tab.href}
        >{$t(tab.key)}</option
      >
    {/each}
  </select>
</div>

<!-- Tab bar visuals (.desktop-tabs/.tab-link/.mobile-tabs) live in
     styles/components.css so Reports.svelte's local-state tab strip can
     reuse the exact same look. -->
