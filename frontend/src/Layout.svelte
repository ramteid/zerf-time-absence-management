<script>
  import { api } from "./api.js";
  import { currentUser, path, go, theme } from "./stores.js";
  import { t, roleLabel } from "./i18n.js";
  import Icon from "./Icons.svelte";

  async function logout() {
    try {
      await api("/auth/logout", { method: "POST" });
    } catch {}
    currentUser.set(false);
    go("/login");
    location.reload();
  }

  $: pathname = (() => {
    const i = $path.indexOf("?");
    return i >= 0 ? $path.slice(0, i) : $path;
  })();
  $: nav = $currentUser?.nav || [];

  // Map nav keys to icon names
  const iconMap = {
    Time: "Clock",
    Absences: "Plane",
    Calendar: "Calendar",
    Account: "User",
    Dashboard: "Home",
    Reports: "BarChart",
    Admin: "Settings",
  };

  // Section grouping
  function navSections(items) {
    const employee = [];
    const lead = [];
    const admin = [];
    for (const link of items) {
      if (link.key === "Dashboard" || link.key === "Reports") {
        lead.push(link);
      } else if (link.key === "Admin") {
        admin.push(link);
      } else {
        employee.push(link);
      }
    }
    return { employee, lead, admin };
  }

  $: sections = navSections(nav);

  function initials(user) {
    return (
      (user.first_name?.[0] || "") + (user.last_name?.[0] || "")
    ).toUpperCase();
  }
</script>

<div class="app-layout">
  <div class="sidebar">
    <div class="sidebar-logo">
      <div class="sidebar-logo-icon">
        <Icon name="Clock" size={16} />
      </div>
      <span class="sidebar-logo-text">KitaZeit</span>
      <button
        class="kz-btn-icon-sm"
        style="color:var(--nav-text-muted);margin-left:auto"
        on:click={theme.toggle}
        title={$theme === "dark" ? $t("Switch to light mode") : $t("Switch to dark mode")}
      >
        <Icon name={$theme === "dark" ? "Sun" : "Moon"} size={15} />
      </button>
    </div>

    <div class="sidebar-nav">
      {#if sections.employee.length}
        <div class="kz-nav-section">{$t("Employee")}</div>
        {#each sections.employee as link}
          <a
            href={link.href}
            data-link="1"
            class="kz-nav-item"
            class:active={pathname === link.href ||
              pathname.startsWith(link.href + "/")}
          >
            <Icon name={iconMap[link.key] || "FileText"} size={17} />
            <span>{$t(link.key)}</span>
          </a>
        {/each}
      {/if}

      {#if sections.lead.length}
        <div class="kz-nav-section" style="margin-top: 8px">{$t("Lead")}</div>
        {#each sections.lead as link}
          <a
            href={link.href}
            data-link="1"
            class="kz-nav-item"
            class:active={pathname === link.href ||
              pathname.startsWith(link.href + "/")}
          >
            <Icon name={iconMap[link.key] || "FileText"} size={17} />
            <span>{$t(link.key)}</span>
          </a>
        {/each}
      {/if}

      {#if sections.admin.length}
        <div class="kz-nav-section" style="margin-top: 8px">{$t("Admin")}</div>
        {#each sections.admin as link}
          <a
            href={link.href}
            data-link="1"
            class="kz-nav-item"
            class:active={link.key === "Admin"
              ? pathname.startsWith("/admin")
              : pathname === link.href || pathname.startsWith(link.href + "/")}
          >
            <Icon name={iconMap[link.key] || "FileText"} size={17} />
            <span>{$t(link.key)}</span>
          </a>
        {/each}
      {/if}
    </div>

    <div class="sidebar-user">
      <div
        class="avatar"
        style="width:30px;height:30px;font-size:11px;background:var(--nav-bg-active);color:var(--nav-text-active)"
      >
        {initials($currentUser)}
      </div>
      <div style="flex:1;min-width:0">
        <div class="sidebar-user-name">
          {$currentUser.first_name}
          {$currentUser.last_name}
        </div>
        <div class="sidebar-user-role">{roleLabel($currentUser.role)}</div>
      </div>
      <button
        class="kz-btn-icon-sm"
        style="color:var(--nav-text-muted)"
        on:click={logout}
        title={$t("Sign out")}
      >
        <Icon name="LogOut" size={15} />
      </button>
    </div>
  </div>

  <div class="main-content">
    <slot />
  </div>
</div>
