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
    { href: "/settings/archived-users", key: "archived_users_tab" },
    { href: "/settings/categories", key: "Categories" },
    { href: "/settings/holidays", key: "Holidays" },
    { href: "/settings/email", key: "Email" },
    { href: "/settings/upload", key: "Nextcloud Backups" },
    { href: "/settings/audit-log", key: "Audit Log" },
  ];

  // The team-settings tab is shown to all leads (including admin leads).
  const teamTab = { href: "/settings/team", key: "Team Settings" };
  const teamUsersTab = { href: "/settings/team-users", key: "Users" };

  $: tabs = isAdmin
    ? [...adminTabs, teamTab]
    : isLead
      ? canManageTeamUsers
        ? [teamUsersTab, teamTab]
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
      <option value={tab.href} selected={pathname === tab.href}>{$t(tab.key)}</option>
    {/each}
  </select>
</div>

<style>
  .desktop-tabs {
    display: flex;
    gap: 2px;
    padding: 0 28px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-surface);
    overflow-x: auto;
  }

  .tab-link {
    padding: 10px 14px;
    font-size: 13px;
    font-weight: 500;
    white-space: nowrap;
    color: var(--text-secondary);
    border-bottom: 2px solid transparent;
    text-decoration: none;
    transition: color 0.12s;
  }

  .tab-link.active {
    color: var(--accent);
    border-bottom-color: var(--accent);
  }

  .mobile-tabs {
    display: none;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-surface);
  }

  .mobile-tabs select {
    width: 100%;
    padding: 9px 36px 9px 12px;
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
    background: var(--bg-canvas);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    appearance: none;
    /* chevron icon via inline SVG background */
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='16' height='16' viewBox='0 0 24 24' fill='none' stroke='%23888' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpolyline points='6 9 12 15 18 9'/%3E%3C/svg%3E");
    background-repeat: no-repeat;
    background-position: right 10px center;
    cursor: pointer;
  }

  .mobile-tabs select:focus {
    outline: none;
    border-color: var(--accent);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 20%, transparent);
  }

  @media (max-width: 640px) {
    .desktop-tabs {
      display: none;
    }
    .mobile-tabs {
      display: block;
    }
  }
</style>
