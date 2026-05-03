<script>
  import { onMount, onDestroy } from "svelte";
  import { api, csrfToken, setUnauthorizedHandler, setGateResetHandler, resetUnauthorizedGate } from "./api.js";
  import {
    currentUser,
    categories,
    settings,
    path,
    go,
    toast,
    toasts,
    notifications,
    notificationsUnread,
    broadcastSession,
    onSessionBroadcast,
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
  let bootNetworkError = false;

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
      bootNetworkError = false;
      if (!$categories.length) {
        try {
          categories.set(await api("/categories"));
        } catch {}
      }
    } catch (err) {
      if (err.isNetworkError) {
        // Don't log out on a network hiccup — keep showing boot screen
        // with a retry option rather than forcing the user to log in again.
        bootNetworkError = true;
      } else {
        currentUser.set(false);
        csrfToken.set(null);
      }
    }
  }

  // Called whenever any API response returns 401/403 outside the auth
  // endpoints. Clears all client state and redirects to login.
  let _sessionExpiredHandling = false;
  function handleSessionExpired() {
    if (_sessionExpiredHandling) return;
    _sessionExpiredHandling = true;
    stopPolling();
    csrfToken.set(null);
    categories.set([]);
    currentUser.set(false);
    go("/", false);
    toast($t("Your session has expired. Please sign in again."), "error");
    // Also call logout to clear the stale cookie.
    fetch("/api/v1/auth/logout", { method: "POST", credentials: "same-origin" }).catch(() => {});
    // Notify other tabs so they also return to login immediately.
    broadcastSession("session-expired");
    // NOTE: _sessionExpiredHandling is intentionally NOT reset here.
    // resetUnauthorizedGate() (called by Login.svelte after successful re-login)
    // also resets this flag via the onGateReset hook registered below.
  }

  // Notification polling: 60s default, paused when tab is hidden. The
  // polling is started/stopped reactively against `currentUser` so it also
  // kicks in after a fresh login (not only when the user was authenticated
  // at app boot) and is torn down on logout.
  let notifTimer = null;
  let visibilityHandler = null;

  async function pollNotifications() {
    if (typeof document !== "undefined" && document.hidden) return;
    try {
      // Use the dedicated counter endpoint so the badge stays accurate
      // even when the user has more than 100 unread notifications (the
      // list endpoint is capped at 100).
      const [list, count] = await Promise.all([
        api("/notifications"),
        api("/notifications/unread-count"),
      ]);
      notifications.set(list);
      notificationsUnread.set(count?.count ?? 0);
    } catch {}
  }

  function startPolling() {
    if (notifTimer) return;
    pollNotifications();
    notifTimer = setInterval(pollNotifications, 60_000);
    if (typeof document !== "undefined" && !visibilityHandler) {
      visibilityHandler = () => {
        if (!document.hidden) pollNotifications();
      };
      document.addEventListener("visibilitychange", visibilityHandler);
    }
  }
  function stopPolling() {
    if (notifTimer) {
      clearInterval(notifTimer);
      notifTimer = null;
    }
    if (visibilityHandler && typeof document !== "undefined") {
      document.removeEventListener("visibilitychange", visibilityHandler);
      visibilityHandler = null;
    }
    notifications.set([]);
    notificationsUnread.set(0);
  }

  $: if (!booting) {
    if ($currentUser) startPolling();
    else stopPolling();
  }

  // Listeners registered in onMount and cleaned up in onDestroy.
  let _unsubBroadcast = null;
  let _focusListener = null;

  async function onFocus() {
    if (!$currentUser) return;
    try {
      const me = await api("/auth/me");
      // Refresh CSRF token in case it rotated while the tab was hidden.
      csrfToken.set(me.csrf_token || null);
    } catch (err) {
      // api("/auth/me") is excluded from the global 401 interceptor to prevent
      // redirect loops during normal boot. So we must handle session expiry
      // explicitly here: if the re-validation call gets a 401/403, treat it
      // as an expired session and trigger the full expiry flow.
      if (!err.isNetworkError) {
        handleSessionExpired();
      }
      // Network errors during tab-focus check are intentionally ignored.
    }
  }

  onMount(async () => {
    setUnauthorizedHandler(handleSessionExpired);
    // When Login.svelte calls resetUnauthorizedGate() after re-login,
    // also reset our local gate so the next session expiry is handled.
    setGateResetHandler(() => { _sessionExpiredHandling = false; });
    await loadSettings();
    await loadMe();
    booting = false;

    // Cross-tab: if another tab logs out or expires, mirror that here immediately.
    _unsubBroadcast = onSessionBroadcast((msg) => {
      if (msg.type === "session-expired" || msg.type === "logout") {
        if ($currentUser) {
          stopPolling();
          csrfToken.set(null);
          categories.set([]);
          currentUser.set(false);
          go("/", false);
          if (msg.type === "session-expired") {
            toast($t("Your session has expired. Please sign in again."), "error");
          }
        }
      }
    });

    // Tab-focus re-validation: silently re-check the session whenever the user
    // returns to this tab after it was hidden/suspended. If the cookie has
    // expired the 401 triggers handleSessionExpired before the user interacts.
    _focusListener = () => { if (!document.hidden) onFocus(); };
    document.addEventListener("visibilitychange", _focusListener);
  });

  onDestroy(() => {
    stopPolling();
    if (_unsubBroadcast) { _unsubBroadcast(); _unsubBroadcast = null; }
    if (_focusListener) {
      document.removeEventListener("visibilitychange", _focusListener);
      _focusListener = null;
    }
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
{:else if bootNetworkError}
  <div style="padding: 2em; text-align: center">
    <p style="color: var(--danger-text); margin-bottom: 1em">
      {$t("Could not reach the server. Please check your connection.")}
    </p>
    <button
      class="kz-btn kz-btn-primary"
      on:click={async () => {
        booting = true;
        bootNetworkError = false;
        await loadMe();
        booting = false;
      }}
    >
      {$t("Retry")}
    </button>
  </div>
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
