<script>
  import { path, currentUser } from "../stores.js";
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
</script>

<div
  class="admin-tabs"
  style="display:flex;flex-wrap:wrap;gap:2px;padding:0 28px;border-bottom:1px solid var(--border);background:var(--bg-surface)"
>
  {#each tabs as tab (tab.href)}
    <a
      href={tab.href}
      data-link="1"
      style="padding:10px 14px;font-size:13px;font-weight:500;white-space:nowrap;color:{pathname ===
      tab.href
        ? 'var(--accent)'
        : 'var(--text-secondary)'};border-bottom:2px solid {pathname ===
      tab.href
        ? 'var(--accent)'
        : 'transparent'};text-decoration:none;transition:color .12s"
    >
      {$t(tab.key)}
    </a>
  {/each}
</div>
