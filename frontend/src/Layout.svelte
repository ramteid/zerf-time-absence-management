<script>
  import { api } from "./api.js";
  import {
    currentUser,
    path,
    go,
    theme,
    notifications,
    notificationsUnread,
    toast,
    broadcastSession,
  } from "./stores.js";
  import {
    clearNotifications,
    markAllNotificationsRead,
    markNotificationRead,
    refreshNotifications,
  } from "./notificationService.js";
  import { t, roleLabel } from "./i18n.js";
  import { fmtDate } from "./format.js";
  import Icon from "./Icons.svelte";

  // Mobile menu
  let mobileMoreOpen = false;

  // Pull-to-refresh
  let pullStartY = 0;
  let pulling = false;
  let pullDistance = 0;
  let refreshing = false;
  const PULL_THRESHOLD = 80;

  function onTouchStart(e) {
    if (window.scrollY === 0 && e.touches.length === 1 && !e.target.closest(".tp-drum")) {
      pullStartY = e.touches[0].clientY;
      pulling = true;
    }
  }
  function onTouchMove(e) {
    if (!pulling) return;
    const dy = e.touches[0].clientY - pullStartY;
    if (dy > 0) {
      pullDistance = Math.min(dy * 0.5, 120);
    } else {
      pulling = false;
      pullDistance = 0;
    }
  }
  async function onTouchEnd() {
    if (!pulling) return;
    if (pullDistance >= PULL_THRESHOLD && !refreshing) {
      refreshing = true;
      pullDistance = PULL_THRESHOLD;
      // reload current page
      await new Promise((r) => setTimeout(r, 300));
      location.reload();
      return;
    }
    pulling = false;
    pullDistance = 0;
  }

  // Bottom nav: show max 4 primary items + "More"
  $: mobileNavItems = (() => {
    const all = nav || [];
    // Priority order for bottom bar
    const primary = ["Dashboard", "Time", "Absences", "Calendar"];
    const shown = primary
      .map((key) => all.find((link) => link.key === key))
      .filter(Boolean)
      .slice(0, 4);
    const shownKeys = new Set(shown.map((link) => link.key));
    const overflow = all.filter((link) => !shownKeys.has(link.key));
    return { shown, overflow };
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

  function notificationTarget(notification) {
    const query = `n=${notification.id}-${Date.now()}`;
    if (
      notification.kind === "timesheet_submitted" ||
      notification.reference_type === "time_entries"
    ) {
      return `/dashboard?focus=timesheets&${query}`;
    }
    if (
      notification.kind === "reopen_request_created" ||
      notification.reference_type === "reopen_request"
    ) {
      return `/dashboard?focus=reopen&${query}`;
    }
    return "";
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
      !e.target.closest(".kz-bell-wrapper") &&
      !e.target.closest(".kz-mobile-bell-wrapper") &&
      !e.target.closest(".kz-notif-panel")
    )
      bellOpen = false;
  }

  $: pathname = (() => {
    const i = $path.indexOf("?");
    return i >= 0 ? $path.slice(0, i) : $path;
  })();
  $: nav = $currentUser?.nav || [];
  $: desktopNav = nav.filter((link) => link.key !== "Account");

  // Map nav keys to icon names
  const iconMap = {
    Time: "Clock",
    Absences: "Plane",
    Calendar: "Calendar",
    Account: "User",
    Dashboard: "Home",
    Reports: "BarChart",
    Admin: "Settings",
    TeamSettings: "Shield",
  };

  // Section grouping
  function navSections(items) {
    const employee = [];
    const lead = [];
    const admin = [];
    for (const link of items) {
      if (
        link.key === "Dashboard" ||
        link.key === "Reports" ||
        link.key === "TeamSettings"
      ) {
        lead.push(link);
      } else if (link.key === "Admin") {
        admin.push(link);
      } else {
        employee.push(link);
      }
    }
    return { employee, lead, admin };
  }

  $: sections = navSections(desktopNav);

  function initials(user) {
    return (
      (user.first_name?.[0] || "") + (user.last_name?.[0] || "")
    ).toUpperCase();
  }
</script>

<svelte:window
  on:click={onDocClick}
  on:touchstart={onTouchStart}
  on:touchmove={onTouchMove}
  on:touchend={onTouchEnd}
/>

<div class="app-layout">
  <!-- Pull-to-refresh indicator -->
  {#if pullDistance > 0}
    <div class="pull-to-refresh" style="height:{pullDistance}px">
      <div class="pull-spinner" class:active={pullDistance >= PULL_THRESHOLD}>
        {#if refreshing}
          <Icon name="Clock" size={20} />
        {:else}
          <span
            style="transform:rotate({pullDistance * 3}deg);display:inline-block"
          >
            ↓
          </span>
        {/if}
      </div>
    </div>
  {/if}
  <div class="sidebar">
    <div class="sidebar-logo">
      <div class="sidebar-logo-icon">
        <Icon name="Clock" size={16} />
      </div>
      <span class="sidebar-logo-text">Zerf</span>
      <div class="kz-bell-wrapper" style="margin-left:auto;position:relative">
        <button
          class="kz-btn-icon-sm"
          style="color:var(--nav-text-muted);position:relative"
          on:click|stopPropagation={toggleBell}
          title={$t("Notifications")}
        >
          <Icon name="Bell" size={15} />
          {#if $notificationsUnread > 0}
            <span
              style="position:absolute;top:-2px;right:-2px;background:var(--danger-text);color:white;border-radius:10px;font-size:9px;padding:1px 4px;line-height:1;min-width:14px;text-align:center;font-weight:600"
            >
              {$notificationsUnread > 99 ? "99+" : $notificationsUnread}
            </span>
          {/if}
        </button>
      </div>
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
      <a
        href="/account"
        data-link="1"
        class="kz-btn-icon-sm"
        class:active={pathname === "/account" || pathname.startsWith("/account/")}
        style="color:var(--nav-text-muted)"
        title={$t("Account")}
      >
        <Icon name="User" size={15} />
      </a>
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

  <!-- Mobile bottom navigation -->
  <nav class="mobile-bottom-nav">
    {#each mobileNavItems.shown as link}
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
    {#if mobileNavItems.overflow.length > 0}
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
          <div
            class="avatar"
            style="width:32px;height:32px;font-size:11px;background:var(--accent);color:white"
          >
            {initials($currentUser)}
          </div>
          <div style="flex:1;min-width:0">
            <div style="font-weight:600;font-size:14px">
              {$currentUser.first_name}
              {$currentUser.last_name}
            </div>
            <div style="font-size:12px;color:var(--text-secondary)">
              {roleLabel($currentUser.role)}
            </div>
          </div>
          <button
            class="kz-btn-icon-sm"
            on:click={() => (mobileMoreOpen = false)}
          >
            <Icon name="X" size={18} />
          </button>
        </div>
        {#each mobileNavItems.overflow as link}
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
        <div
          style="border-top:1px solid var(--border);margin-top:8px;padding-top:8px"
        >
          <button class="mobile-more-item" on:click={theme.toggle}>
            <Icon name={$theme === "dark" ? "Sun" : "Moon"} size={18} />
            <span
              >{$theme === "dark"
                ? $t("Switch to light mode")
                : $t("Switch to dark mode")}</span
            >
          </button>
          <button
            class="mobile-more-item"
            style="color:var(--danger-text)"
            on:click={logout}
          >
            <Icon name="LogOut" size={18} />
            <span>{$t("Sign out")}</span>
          </button>
        </div>
      </div>
    </div>
  {/if}

  <!-- Mobile bell button: fixed top-right, visible only on mobile -->
  <div class="kz-mobile-bell-wrapper">
    <button
      class="kz-btn-icon-sm"
      style="color:var(--text-secondary);position:relative;background:var(--bg-surface);border:1px solid var(--border);border-radius:8px;padding:6px;box-shadow:var(--shadow-md)"
      on:click|stopPropagation={toggleBell}
      title={$t("Notifications")}
    >
      <Icon name="Bell" size={18} />
      {#if $notificationsUnread > 0}
        <span
          style="position:absolute;top:-4px;right:-4px;background:var(--danger-text);color:white;border-radius:10px;font-size:9px;padding:1px 4px;line-height:1;min-width:14px;text-align:center;font-weight:600"
        >
          {$notificationsUnread > 99 ? "99+" : $notificationsUnread}
        </span>
      {/if}
    </button>
  </div>

  <!-- Notification panel -->
  {#if bellOpen}
    <!-- svelte-ignore a11y-no-noninteractive-element-interactions -->
    <div
      class="kz-notif-panel"
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
          class="kz-btn kz-btn-sm kz-btn-ghost"
          on:click={markAllRead}
          disabled={$notificationsUnread === 0}
          title={$t("Mark all as read")}
          style="font-size:11px"
        >
          <Icon name="Check" size={12} />
        </button>
        <button
          class="kz-btn kz-btn-sm kz-btn-ghost"
          on:click={clearAll}
          disabled={$notifications.length === 0}
          title={$t("Clear all")}
          style="font-size:11px"
        >
          <Icon name="X" size={12} />
        </button>
        <button
          class="kz-btn kz-btn-sm kz-btn-ghost"
          on:click={() => (bellOpen = false)}
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
        {#each $notifications as n}
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
            <div style="font-size:12.5px;font-weight:500">{n.title}</div>
            {#if n.body}
              <div
                style="font-size:11.5px;color:var(--text-secondary);margin-top:2px;line-height:1.4"
              >
                {n.body}
              </div>
            {/if}
            <div
              class="tab-num"
              style="font-size:10.5px;color:var(--text-tertiary);margin-top:4px"
            >
              {fmtDate(n.created_at)}
            </div>
          </div>
        {/each}
      {/if}
    </div>
  {/if}
</div>
