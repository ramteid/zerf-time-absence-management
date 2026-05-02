<script>
  import { onMount, onDestroy } from "svelte";
  import { api, csrfToken } from "./api.js";
  import {
    currentUser,
    categories,
    settings,
    path,
    go,
    toasts,
    notifications,
    notificationsUnread,
  } from "./stores.js";
  import { setLanguage, t } from "./i18n.js";
  import Layout from "./Layout.svelte";
  import Login from "./routes/Login.svelte";
  import Time from "./routes/Time.svelte";
  import Absences from "./routes/Absences.svelte";
  import Calendar from "./routes/Calendar.svelte";
  import Account from "./routes/Account.svelte";
  import Dashboard from "./routes/Dashboard.svelte";
  import Reports from "./routes/Reports.svelte";
  import AdminUsers from "./routes/AdminUsers.svelte";
  import AdminCategories from "./routes/AdminCategories.svelte";
  import AdminHolidays from "./routes/AdminHolidays.svelte";
  import AdminAuditLog from "./routes/AdminAuditLog.svelte";
  import AdminSettings from "./routes/AdminSettings.svelte";
  import AdminTabs from "./routes/AdminTabs.svelte";
  import TeamPolicy from "./routes/TeamPolicy.svelte";
  import NotFound from "./routes/NotFound.svelte";

  let booting = true;

  async function loadSettings() {
    try {
      const s = await api("/settings/public");
      settings.set(s);
      if (s.ui_language) setLanguage(s.ui_language);
    } catch {}
  }

  async function loadMe() {
    try {
      const me = await api("/auth/me");
      currentUser.set(me);
      csrfToken.set(me.csrf_token || null);
      if (!$categories.length) {
        try {
          categories.set(await api("/categories"));
        } catch {}
      }
    } catch {
      currentUser.set(false);
    }
  }

  // Notification polling: 60s default, paused when tab is hidden.
  let notifTimer = null;
  async function pollNotifications() {
    if (!$currentUser || $currentUser === false) return;
    if (typeof document !== "undefined" && document.hidden) return;
    try {
      const list = await api("/notifications");
      notifications.set(list);
      notificationsUnread.set(list.filter((n) => !n.is_read).length);
    } catch {}
  }

  onMount(async () => {
    await loadSettings();
    await loadMe();
    booting = false;
    if ($currentUser) {
      pollNotifications();
      notifTimer = setInterval(pollNotifications, 60_000);
      if (typeof document !== "undefined") {
        document.addEventListener("visibilitychange", () => {
          if (!document.hidden) pollNotifications();
        });
      }
    }
  });

  onDestroy(() => {
    if (notifTimer) clearInterval(notifTimer);
  });

  $: pathname = (() => {
    const idx = $path.indexOf("?");
    return idx >= 0 ? $path.slice(0, idx) : $path;
  })();

  $: route = matchRoute(pathname, $currentUser);
  $: isAdmin = pathname.startsWith("/admin");

  function matchRoute(p, user) {
    if (p === "/" || p === "") {
      if (user && user.home) {
        go(user.home, false);
        return null;
      }
    }
    if (!user) return null;
    if (user.must_change_password && p !== "/account") {
      go("/account", false);
      return null;
    }
    const map = {
      "/time": Time,
      "/absences": Absences,
      "/calendar": Calendar,
      "/account": Account,
      "/dashboard": Dashboard,
      "/reports": Reports,
      "/admin": AdminUsers,
      "/admin/users": AdminUsers,
      "/admin/categories": AdminCategories,
      "/admin/holidays": AdminHolidays,
      "/admin/audit-log": AdminAuditLog,
      "/admin/settings": AdminSettings,
      "/team-policy": TeamPolicy,
    };
    return map[p] || NotFound;
  }

  // Intercept data-link clicks
  function onClick(e) {
    const a = e.target.closest("a[data-link]");
    if (a) {
      e.preventDefault();
      go(a.getAttribute("href"));
    }
  }
</script>

<svelte:window on:click={onClick} />

{#if booting}
  <p style="padding: 2em">{$t("Loading...")}</p>
{:else if !$currentUser}
  <Login />
{:else if route}
  <Layout>
    {#if isAdmin}
      <AdminTabs />
    {/if}
    <svelte:component this={route} />
  </Layout>
{:else}
  <p style="padding: 2em">{$t("Loading...")}</p>
{/if}

<div class="toast-container">
  {#each $toasts as item (item.id)}
    <div class="toast toast-{item.type}">{item.message}</div>
  {/each}
</div>
