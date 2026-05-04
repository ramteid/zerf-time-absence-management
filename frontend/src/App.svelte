<script>
  import { onMount, onDestroy } from "svelte";
  import {
    api,
    csrfToken,
    setUnauthorizedHandler,
    setGateResetHandler,
    resetUnauthorizedGate,
  } from "./api.js";
  import {
    currentUser,
    categories,
    settings,
    theme,
    path,
    go,
    toast,
    toasts,
    broadcastSession,
    onSessionBroadcast,
  } from "./stores.js";
  import {
    startNotifications,
    stopNotifications,
  } from "./notificationService.js";
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
  import TeamSettings from "./routes/TeamSettings.svelte";
  import NotFound from "./routes/NotFound.svelte";

  let booting = true;
  let bootNetworkError = false;

  function debugLog(event, data = {}) {
    console.debug("[app-debug]", event, {
      path: $path,
      pathname,
      hasUser: !!$currentUser,
      userId: $currentUser?.id ?? null,
      booting,
      bootNetworkError,
      ...data,
    });
  }

  async function loadSettings() {
    try {
      const s = await api("/settings/public");
      settings.set(s);
      if (s.ui_language) setLanguage(s.ui_language);
    } catch {}
  }

  async function loadMe() {
    debugLog("loadMe:start");
    try {
      const me = await api("/auth/me");
      debugLog("loadMe:success", {
        meId: me?.id ?? null,
        meHome: me?.home ?? null,
        mustChangePassword: !!me?.must_change_password,
      });
      currentUser.set(me);
      csrfToken.set(me.csrf_token || null);
      theme.set(me.dark_mode ? "dark" : "light");
      bootNetworkError = false;
      if (!$categories.length) {
        try {
          categories.set(await api("/categories"));
          debugLog("loadMe:categories-loaded");
        } catch (e) {
          debugLog("loadMe:categories-failed", { message: e?.message ?? null });
          toast(
            $t("Failed to load categories. Some features may be unavailable."),
            "error",
          );
        }
      }
    } catch (err) {
      debugLog("loadMe:error", {
        message: err?.message ?? null,
        isNetworkError: !!err?.isNetworkError,
      });
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
    debugLog("sessionExpired:handle", {
      alreadyHandling: _sessionExpiredHandling,
    });
    if (_sessionExpiredHandling) return;
    _sessionExpiredHandling = true;
    stopNotifications();
    csrfToken.set(null);
    categories.set([]);
    currentUser.set(false);
    go("/", false);
    toast($t("Your session has expired. Please sign in again."), "error");
    // Also call logout to clear the stale cookie.
    fetch("/api/v1/auth/logout", {
      method: "POST",
      credentials: "same-origin",
    }).catch(() => {});
    // Notify other tabs so they also return to login immediately.
    broadcastSession("session-expired");
    // NOTE: _sessionExpiredHandling is intentionally NOT reset here.
    // resetUnauthorizedGate() (called by Login.svelte after successful re-login)
    // also resets this flag via the onGateReset hook registered below.
  }

  $: if (!booting) {
    if ($currentUser) startNotifications();
    else stopNotifications();
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
      // Sync dark mode preference in case it changed on another device.
      theme.set(me.dark_mode ? "dark" : "light");
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
    setGateResetHandler(() => {
      _sessionExpiredHandling = false;
    });
    await loadSettings();
    await loadMe();
    booting = false;

    // Cross-tab: if another tab logs out or expires, mirror that here immediately.
    _unsubBroadcast = onSessionBroadcast((msg) => {
      debugLog("sessionBroadcast:received", {
        type: msg?.type ?? null,
      });
      if (msg.type === "session-expired" || msg.type === "logout") {
        if ($currentUser) {
          stopNotifications();
          csrfToken.set(null);
          categories.set([]);
          currentUser.set(false);
          go("/", false);
          if (msg.type === "session-expired") {
            toast(
              $t("Your session has expired. Please sign in again."),
              "error",
            );
          }
        }
      }
    });

    // Tab-focus re-validation: silently re-check the session whenever the user
    // returns to this tab after it was hidden/suspended. If the cookie has
    // expired the 401 triggers handleSessionExpired before the user interacts.
    _focusListener = () => {
      if (!document.hidden) onFocus();
    };
    document.addEventListener("visibilitychange", _focusListener);
  });

  onDestroy(() => {
    stopNotifications();
    if (_unsubBroadcast) {
      _unsubBroadcast();
      _unsubBroadcast = null;
    }
    if (_focusListener) {
      document.removeEventListener("visibilitychange", _focusListener);
      _focusListener = null;
    }
  });

  $: pathname = (() => {
    const idx = $path.indexOf("?");
    return idx >= 0 ? $path.slice(0, idx) : $path;
  })();

  const routeMap = {
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
    "/team-settings": TeamSettings,
  };

  const routeAccess = {
    "/dashboard": (user) => !!user?.permissions?.can_view_dashboard,
    "/reports": (user) => !!user?.permissions?.can_view_team_reports,
    "/team-settings": (user) => !!user?.permissions?.can_manage_team_settings,
    "/admin": (user) => !!user?.permissions?.can_manage_users,
    "/admin/users": (user) => !!user?.permissions?.can_manage_users,
    "/admin/categories": (user) => !!user?.permissions?.can_manage_categories,
    "/admin/holidays": (user) => !!user?.permissions?.can_manage_holidays,
    "/admin/audit-log": (user) => !!user?.permissions?.can_view_audit_log,
    "/admin/settings": (user) => !!user?.permissions?.can_manage_settings,
  };

  $: route = resolveRoute(pathname, $currentUser);
  $: isAdmin =
    pathname.startsWith("/admin") &&
    !!$currentUser?.permissions?.can_manage_users;

  function preferredHome(user) {
    const dashboardAvailable = (user?.nav || []).some(
      (item) => item?.key === "Dashboard" || item?.href === "/dashboard",
    );
    return user?.home && user.home !== "/" && user.home !== ""
      ? user.home
      : dashboardAvailable
        ? "/dashboard"
        : "/time";
  }

  function canAccessRoute(path, user) {
    const check = routeAccess[path];
    return check ? check(user) : true;
  }

  function resolveRoute(p, user) {
    debugLog("route:resolve", {
      inputPath: p,
      userHome: user?.home ?? null,
      mustChangePassword: !!user?.must_change_password,
      mustConfigureSettings: !!user?.must_configure_settings,
    });
    if (!user) return null;

    // Resolve redirects without side-effects — just return the target component
    // directly so the reactive chain never yields null for a logged-in user.
    if (p === "/" || p === "") {
      const dest = user.must_change_password
        ? "/account"
        : user.must_configure_settings
          ? "/admin/settings"
          : preferredHome(user);
      debugLog("route:redirect-home", { dest });
      // Update the URL bar (deferred so we don't mutate stores mid-reactive-cycle)
      setTimeout(() => go(dest, false), 0);
      return routeMap[dest] || NotFound;
    }
    if (user.must_change_password && p !== "/account") {
      debugLog("route:redirect-password-change");
      setTimeout(() => go("/account", false), 0);
      return Account;
    }
    // Only redirect to settings setup when the password is already in order,
    // so an admin with both flags can complete the password change first.
    if (
      user.must_configure_settings &&
      !user.must_change_password &&
      p !== "/admin/settings"
    ) {
      debugLog("route:redirect-configure-settings");
      setTimeout(() => go("/admin/settings", false), 0);
      return AdminSettings;
    }
    if (routeMap[p] && !canAccessRoute(p, user)) {
      const dest = preferredHome(user);
      debugLog("route:redirect-unauthorized", {
        inputPath: p,
        dest,
      });
      setTimeout(() => go(dest, false), 0);
      return routeMap[dest] || NotFound;
    }
    const resolved = routeMap[p] || NotFound;
    debugLog("route:resolved", {
      inputPath: p,
      resolved: resolved?.name ?? "anonymous-component",
    });
    return resolved;
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
