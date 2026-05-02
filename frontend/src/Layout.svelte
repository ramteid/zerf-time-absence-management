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
  } from "./stores.js";
  import { t, roleLabel } from "./i18n.js";
  import { fmtDate } from "./format.js";
  import Icon from "./Icons.svelte";

  async function logout() {
    try {
      await api("/auth/logout", { method: "POST" });
    } catch {}
    currentUser.set(false);
    go("/login");
    location.reload();
  }

  let bellOpen = false;
  function toggleBell() {
    bellOpen = !bellOpen;
    if (bellOpen) {
      // Refresh on open so the list is current.
      reloadNotifications();
    }
  }
  async function reloadNotifications() {
    try {
      const [list, count] = await Promise.all([
        api("/notifications"),
        api("/notifications/unread-count"),
      ]);
      notifications.set(list);
      notificationsUnread.set(count?.count ?? 0);
    } catch {}
  }
  async function markRead(n) {
    if (n.is_read) return;
    try {
      await api(`/notifications/${n.id}/read`, { method: "POST", body: {} });
      n.is_read = true;
      notifications.update((arr) => arr.slice());
      notificationsUnread.update((c) => Math.max(0, c - 1));
    } catch {}
  }
  async function markAllRead() {
    try {
      await api("/notifications/read-all", { method: "POST", body: {} });
      notifications.update((arr) => arr.map((n) => ({ ...n, is_read: true })));
      notificationsUnread.set(0);
    } catch (e) {
      toast(e.message || $t("Error"), "err");
    }
  }
  async function clearAll() {
    try {
      await api("/notifications", { method: "DELETE" });
      notifications.set([]);
      notificationsUnread.set(0);
    } catch (e) {
      toast(e.message || $t("Error"), "err");
    }
  }
  function onDocClick(e) {
    if (!bellOpen) return;
    if (!e.target.closest(".kz-bell-wrapper")) bellOpen = false;
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
    TeamPolicy: "Shield",
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
        link.key === "TeamPolicy"
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

  $: sections = navSections(nav);

  function initials(user) {
    return (
      (user.first_name?.[0] || "") + (user.last_name?.[0] || "")
    ).toUpperCase();
  }
</script>

<svelte:window on:click={onDocClick} />

<div class="app-layout">
  <div class="sidebar">
    <div class="sidebar-logo">
      <div class="sidebar-logo-icon">
        <Icon name="Clock" size={16} />
      </div>
      <span class="sidebar-logo-text">KitaZeit</span>
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
        {#if bellOpen}
          <!-- svelte-ignore a11y-no-noninteractive-element-interactions -->
          <div
            style="position:absolute;top:28px;right:0;width:320px;max-height:480px;overflow:auto;background:var(--bg-surface);border:1px solid var(--border);border-radius:8px;box-shadow:0 4px 16px rgba(0,0,0,.18);z-index:200"
            on:click|stopPropagation
            on:keydown={() => {}}
            role="dialog"
            tabindex="-1"
          >
            <div
              style="padding:8px 12px;display:flex;align-items:center;gap:6px;border-bottom:1px solid var(--border)"
            >
              <strong style="flex:1;font-size:13px"
                >{$t("Notifications")}</strong
              >
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
                  on:click={() => markRead(n)}
                  on:keydown={() => {}}
                  role="button"
                  tabindex="0"
                  style="padding:10px 12px;border-bottom:1px solid var(--border);cursor:pointer;background:{n.is_read
                    ? 'transparent'
                    : 'var(--bg-elevated, rgba(0,0,0,.03))'}"
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
      <button
        class="kz-btn-icon-sm"
        style="color:var(--nav-text-muted);margin-left:4px"
        on:click={theme.toggle}
        title={$theme === "dark"
          ? $t("Switch to light mode")
          : $t("Switch to dark mode")}
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
