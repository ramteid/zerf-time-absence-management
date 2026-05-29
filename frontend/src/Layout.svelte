<script>
  import { api } from "./api.js";
  import {
    currentUser,
    path,
    go,
    notifications,
    notificationsUnread,
    toast,
    broadcastSession,
    settings,
  } from "./stores.js";
  import {
    clearNotifications,
    markAllNotificationsRead,
    markNotificationRead,
    refreshNotifications,
  } from "./notificationService.js";
  import { t, language, roleLabel, renderNotification } from "./i18n.js";
  import { fmtDateTime } from "./format.js";
  import Icon from "./Icons.svelte";
  import AppLogo from "./AppLogo.svelte";
  import PullToRefresh from "./lib/ui/PullToRefresh.svelte";
  import { notificationTarget } from "./lib/domain/dashboard.js";

  // Mobile menu
  let mobileMoreOpen = false;

  // Bottom nav: show max 4 primary items + "More"
  $: mobileNavItems = (() => {
    const account = (nav || []).find((link) => link.key === "Account");
    const all = (nav || []).filter((link) => link.key !== "Account");
    // Priority order for bottom bar
    const primary = ["Dashboard", "Time", "Absences", "Calendar"];
    const shown = primary
      .map((key) => all.find((link) => link.key === key))
      .filter(Boolean)
      .slice(0, 4);
    const shownKeys = new Set(shown.map((link) => link.key));
    const overflow = all.filter((link) => !shownKeys.has(link.key));
    return { shown, overflow, account };
  })();

  async function logout() {
    try {
      await api("/auth/logout", { method: "POST" });
    } catch {}
    currentUser.set(false);
    go("/", false);
    // Tell every other open tab to also return to login.
    broadcastSession("logout");
  }

  let bellOpen = false;
  function toggleBell() {
    bellOpen = !bellOpen;
    if (bellOpen) {
      // Close the mobile more sheet if open.
      mobileMoreOpen = false;
      // Refresh on open so the list is current.
      refreshNotifications().catch(() => {});
    }
  }

  async function openNotification(notification) {
    bellOpen = false;
    try {
      await markNotificationRead(notification);
    } catch {}

    const target = notificationTarget(notification);
    if (target) {
      go(target);
    }
  }

  async function markAllRead() {
    try {
      await markAllNotificationsRead();
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
    }
  }
  async function clearAll() {
    try {
      await clearNotifications();
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
    }
  }
  function onDocClick(e) {
    if (!bellOpen) return;
    if (
      !e.target.closest(".zf-bell-wrapper") &&
      !e.target.closest(".zf-mobile-bell-wrapper") &&
      !e.target.closest(".zf-notif-panel")
    )
      bellOpen = false;
  }

  $: pathname = (() => {
    const queryIndex = $path.indexOf("?");
    return queryIndex >= 0 ? $path.slice(0, queryIndex) : $path;
  })();
  $: nav = $currentUser?.nav || [];
  $: desktopNav = nav.filter((link) => link.key !== "Account");

  // Map nav keys to icon names
  const iconMap = {
    Time: "Clock",
    Absences: "Plane",
    Calendar: "Calendar",
    Dashboard: "Home",
    Reports: "BarChart",
    Admin: "Settings",
    TeamSettings: "Shield",
  };

  // Section grouping
  function navSections(items) {
    const dashboard = [];
    const employee = [];
    const lead = [];
    const admin = [];
    for (const link of items) {
      if (link.key === "Dashboard") {
        dashboard.push(link);
      } else if (link.key === "TeamSettings") {
        lead.push(link);
      } else if (link.key === "Admin") {
        admin.push(link);
      } else {
        employee.push(link);
      }
    }
    return { dashboard, employee, lead, admin };
  }

  $: sections = navSections(desktopNav);

  function initials(user) {
    return (
      (user.first_name?.[0] || "") + (user.last_name?.[0] || "")
    ).toUpperCase();
  }
</script>

<svelte:window on:click={onDocClick} />

<div class="app-layout">
  <PullToRefresh />
  <!-- Mobile title bar: logo + notification icon (mobile only) -->
  <div class="mobile-title-bar">
    <div class="mobile-title-bar-brand">
      <div class="mobile-title-bar-logo"><AppLogo size={30} /></div>
      <div class="mobile-title-bar-names">
        <span class="mobile-title-bar-appname">{$t("Time tracking")}</span>
        {#if $settings?.organization_name}
          <span class="mobile-title-bar-org">{$settings.organization_name}</span>
        {/if}
      </div>
    </div>
    <div class="zf-mobile-bell-wrapper mobile-title-bar-bell">
      <button
        class="zf-btn-icon-sm"
        style="color:var(--nav-text-muted);position:relative"
        on:click|stopPropagation={toggleBell}
        title={$t("Notifications")}
      >
        <Icon name="Bell" size={21} />
        {#if $notificationsUnread > 0}
          <span
            style="position:absolute;top:-4px;right:-4px;background:var(--danger-text);color:white;border-radius:10px;font-size:9px;padding:1px 4px;line-height:1;min-width:14px;text-align:center;font-weight:400"
          >
            {$notificationsUnread > 99 ? "99+" : $notificationsUnread}
          </span>
        {/if}
      </button>
    </div>
  </div>

  <div class="sidebar">
    <div class="sidebar-logo">
      <div class="sidebar-logo-icon"><AppLogo size={30} /></div>
      <div style="display:flex;flex-direction:column;line-height:1.2;min-width:0;flex:1">
        <span class="sidebar-logo-text">{$t("Time tracking")}</span>
        {#if $settings?.organization_name}
          <span style="font-size:12px;color:var(--nav-text-muted);word-break:break-word">{$settings.organization_name}</span>
        {/if}
      </div>
    </div>

    <div class="sidebar-nav">
      {#each sections.dashboard as link (link.href)}
        <a
          href={link.href}
          data-link="1"
          class="zf-nav-item"
          class:active={pathname === link.href ||
            pathname.startsWith(link.href + "/")}
        >
          <Icon name={iconMap[link.key] || "FileText"} size={17} />
          <span>{$t(link.key)}</span>
        </a>
      {/each}

      {#if sections.employee.length}
        <div class="zf-nav-section" style={sections.dashboard.length ? "margin-top: 8px" : ""}>{$t("Employee")}</div>
        {#each sections.employee as link (link.href)}
          <a
            href={link.href}
            data-link="1"
            class="zf-nav-item"
            class:active={pathname === link.href ||
              pathname.startsWith(link.href + "/")}
          >
            <Icon name={iconMap[link.key] || "FileText"} size={17} />
            <span>{$t(link.key)}</span>
          </a>
        {/each}
      {/if}

      {#if sections.lead.length}
        <div class="zf-nav-section" style="margin-top: 8px">{$t("Lead")}</div>
        {#each sections.lead as link (link.href)}
          <a
            href={link.href}
            data-link="1"
            class="zf-nav-item"
            class:active={pathname === link.href ||
              pathname.startsWith(link.href + "/")}
          >
            <Icon name={iconMap[link.key] || "FileText"} size={17} />
            <span>{$t(link.key)}</span>
          </a>
        {/each}
      {/if}

      {#if sections.admin.length}
        <div class="zf-nav-section" style="margin-top: 8px">{$t("Admin")}</div>
        {#each sections.admin as link (link.href)}
          <a
            href={link.href}
            data-link="1"
            class="zf-nav-item"
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
      <a
        href="/account"
        data-link="1"
        class="sidebar-user-account"
        class:active={pathname === "/account" ||
          pathname.startsWith("/account/")}
        title={$t("Account")}
        aria-label={$t("Account")}
      >
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
      </a>
      <div class="zf-bell-wrapper" style="position:relative">
        <button
          class="zf-btn-icon-sm"
          style="color:var(--nav-text-muted);position:relative"
          on:click|stopPropagation={toggleBell}
          title={$t("Notifications")}
        >
          <Icon name="Bell" size={17} />
          {#if $notificationsUnread > 0}
            <span
              style="position:absolute;top:-2px;right:-2px;background:var(--danger-text);color:white;border-radius:10px;font-size:9px;padding:1px 4px;line-height:1;min-width:14px;text-align:center;font-weight:400"
            >
              {$notificationsUnread > 99 ? "99+" : $notificationsUnread}
            </span>
          {/if}
        </button>
      </div>
      <button
        class="zf-btn-icon-sm"
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

  <!-- Mobile bottom navigation -->
  <nav class="mobile-bottom-nav">
    {#each mobileNavItems.shown as link (link.href)}
      <a
        href={link.href}
        data-link="1"
        class="mobile-nav-item"
        class:active={pathname === link.href ||
          pathname.startsWith(link.href + "/")}
      >
        <Icon name={iconMap[link.key] || "FileText"} size={20} />
        <span>{$t(link.key)}</span>
      </a>
    {/each}
    {#if mobileNavItems.overflow.length > 0 || mobileNavItems.account}
      <button
        class="mobile-nav-item"
        class:active={mobileMoreOpen}
        on:click|stopPropagation={() => (mobileMoreOpen = !mobileMoreOpen)}
      >
        <Icon name="Menu" size={20} />
        <span>{$t("More")}</span>
      </button>
    {/if}
  </nav>

  <!-- Mobile "More" overlay -->
  {#if mobileMoreOpen}
    <!-- svelte-ignore a11y-click-events-have-key-events -->
    <!-- svelte-ignore a11y-no-static-element-interactions -->
    <div class="mobile-more-overlay" on:click={() => (mobileMoreOpen = false)}>
      <div class="mobile-more-sheet" on:click|stopPropagation>
        <div class="mobile-more-header">
          <a
            href={mobileNavItems.account?.href || "/account"}
            data-link="1"
            on:click={() => (mobileMoreOpen = false)}
            style="display:flex;align-items:center;gap:12px;flex:1;min-width:0;color:inherit;text-decoration:none;border-radius:8px"
          >
            <div
              class="avatar"
              style="width:32px;height:32px;font-size:11px;background:var(--accent);color:white"
            >
              {initials($currentUser)}
            </div>
            <div style="flex:1;min-width:0">
              <div style="font-weight:400;font-size:14px">
                {$currentUser.first_name}
                {$currentUser.last_name}
              </div>
              <div style="font-size:12px;color:var(--text-secondary)">
                {roleLabel($currentUser.role)}
              </div>
            </div>
          </a>
          <button
            class="zf-btn zf-btn-sm zf-btn-ghost zf-btn-danger"
            on:click={logout}
          >
            <Icon name="LogOut" size={15} />
            {$t("Sign out")}
          </button>
          <button
            class="zf-btn-icon-sm"
            on:click={() => (mobileMoreOpen = false)}
          >
            <Icon name="X" size={18} />
          </button>
        </div>
        {#each mobileNavItems.overflow as link (link.href)}
          <a
            href={link.href}
            data-link="1"
            class="mobile-more-item"
            class:active={link.key === "Admin"
              ? pathname.startsWith("/admin")
              : pathname === link.href || pathname.startsWith(link.href + "/")}
            on:click={() => (mobileMoreOpen = false)}
          >
            <Icon name={iconMap[link.key] || "FileText"} size={18} />
            <span>{$t(link.key)}</span>
          </a>
        {/each}
      </div>
    </div>
  {/if}

  <!-- Notification panel -->
  {#if bellOpen}
    <div
      class="zf-notif-panel"
      on:click|stopPropagation
      on:keydown={() => {}}
      role="dialog"
      tabindex="-1"
    >
      <div
        style="padding:8px 12px;display:flex;align-items:center;gap:6px;border-bottom:1px solid var(--border)"
      >
        <strong style="flex:1;font-size:13px">{$t("Notifications")}</strong>
        <button
          class="zf-btn zf-btn-sm zf-btn-ghost"
          on:click={markAllRead}
          disabled={$notificationsUnread === 0}
          title={$t("Mark all as read")}
          aria-label={$t("Mark all as read")}
          style="font-size:11px"
        >
          <Icon name="Check" size={12} />
        </button>
        <button
          class="zf-btn zf-btn-sm zf-btn-ghost zf-btn-danger"
          on:click={clearAll}
          disabled={$notifications.length === 0}
          title={$t("Clear all")}
          aria-label={$t("Clear all")}
          style="font-size:11px"
        >
          <Icon name="Trash" size={12} />
        </button>
        <button
          class="zf-btn zf-btn-sm zf-btn-ghost"
          on:click={() => (bellOpen = false)}
          title={$t("Close")}
          aria-label={$t("Close")}
          style="font-size:11px"
        >
          <Icon name="X" size={14} />
        </button>
      </div>
      {#if $notifications.length === 0}
        <div
          style="padding:24px;text-align:center;color:var(--text-tertiary);font-size:12px"
        >
          {$t("No notifications.")}
        </div>
      {:else}
        {#each $notifications as n (n.id)}
          {@const rendered = renderNotification(n, $language)}
          <div
            on:click={() => openNotification(n)}
            on:keydown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                openNotification(n);
              }
            }}
            role="button"
            tabindex="0"
            style="padding:10px 12px;border-bottom:1px solid var(--border);cursor:pointer;background:{n.is_read
              ? 'transparent'
              : 'var(--accent-soft)'}"
          >
            <div style="font-size:12.5px;font-weight:500">{rendered.title}</div>
            {#if rendered.body}
              <div
                style="font-size:11.5px;color:var(--text-secondary);margin-top:2px;line-height:1.4"
              >
                {rendered.body}
              </div>
            {/if}
            <div
              class="tab-num"
              style="font-size:10.5px;color:var(--text-tertiary);margin-top:4px"
            >
              {fmtDateTime(n.created_at)}
            </div>
          </div>
        {/each}
      {/if}
    </div>
  {/if}
</div>
