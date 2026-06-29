<script>
  import { onMount, onDestroy } from "svelte";
  import {
    api,
    csrfToken,
    setUnauthorizedHandler,
    setGateResetHandler,
  } from "./api.js";
  import {
    currentUser,
    categories,
    absenceCategories,
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
  import { setLanguage, t, setAbsenceCategoryCache } from "./i18n.js";
  import { loadPostAuthData } from "./appData.js";
  import Layout from "./Layout.svelte";
  import Login from "./routes/Login.svelte";
  import Setup from "./routes/Setup.svelte";
  import SettingsTabs from "./routes/SettingsTabs.svelte";

  let booting = true;
  let bootNetworkError = false;
  let needsSetup = false;
  let setupEmail = "";

  function debugLog(event, data = {}) {
    // Gated behind the debug-build flag so production bundles emit no console
    // output (and never disclose user ids / paths). The whole body is removed
    // by dead-code elimination when __ZERF_DEBUG__ is false.
    if (!__ZERF_DEBUG__) return;
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
      const publicSettings = await api("/settings/public");
      if (!publicSettings.time_format) publicSettings.time_format = "24h";
      if (!publicSettings.timezone) publicSettings.timezone = "Europe/Berlin";
      settings.set(publicSettings);
      if (publicSettings.ui_language) setLanguage(publicSettings.ui_language);
    } catch {}
  }

  async function loadMe() {
    debugLog("loadMe:start");
    try {
      const currentUserResponse = await api("/auth/me");
      debugLog("loadMe:success", {
        meId: currentUserResponse?.id ?? null,
        meHome: currentUserResponse?.home ?? null,
        mustChangePassword: !!currentUserResponse?.must_change_password,
      });
      currentUser.set(currentUserResponse);
      csrfToken.set(currentUserResponse.csrf_token || null);
      theme.set(currentUserResponse.dark_mode ? "dark" : "light");
      bootNetworkError = false;
      // Skip data endpoints while the user must still change their password —
      // the middleware blocks them with 403, which would trigger a spurious
      // error toast before the password-change screen even appears.
      if (!currentUserResponse.must_change_password) {
        await loadPostAuthData();
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
    absenceCategories.set([]);
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

  // Keep the i18n label cache in sync with the store so absenceKindLabel
  // always has the latest DB-configured category names without importing
  // the store directly in i18n.js (which causes module isolation issues
  // in Vitest when svelte is mocked).
  $: setAbsenceCategoryCache($absenceCategories);

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
      const validatedUser = await api("/auth/me");
      // Refresh the full user object so that permission changes made by an
      // admin while this tab was in the background (e.g. enabling
      // allow_team_lead_manage_assistants) are reflected immediately without
      // requiring a manual page reload.
      currentUser.set(validatedUser);
      // Refresh CSRF token in case it rotated while the tab was hidden.
      csrfToken.set(validatedUser.csrf_token || null);
      // Sync dark mode preference in case it changed on another device.
      theme.set(validatedUser.dark_mode ? "dark" : "light");
    } catch (error) {
      // api("/auth/me") is excluded from the global 401 interceptor to prevent
      // redirect loops during normal boot. So we must handle session expiry
      // explicitly here: if the re-validation call gets a 401/403, treat it
      // as an expired session and trigger the full expiry flow.
      if (!error.isNetworkError) {
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
    try {
      const status = await api("/auth/setup-status");
      needsSetup = !!status?.needs_setup;
    } catch {}
    if (!needsSetup) {
      await loadMe();
    }
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

  const routeLoaders = {
    "/time": () => import("./routes/Time.svelte"),
    "/absences": () => import("./routes/Absences.svelte"),
    "/calendar": () => import("./routes/Calendar.svelte"),
    "/account": () => import("./routes/Account.svelte"),
    "/dashboard": () => import("./routes/Dashboard.svelte"),
    "/reports": () => import("./routes/Reports.svelte"),
    "/settings/general": () => import("./routes/AdminSettings.svelte"),
    "/settings/users": () => import("./routes/AdminUsers.svelte"),
    "/settings/archived-users": () => import("./routes/AdminArchivedUsers.svelte"),
    "/settings/categories": () => import("./routes/AdminCategories.svelte"),
    "/settings/holidays": () => import("./routes/AdminHolidays.svelte"),
    "/settings/audit-log": () => import("./routes/AdminAuditLog.svelte"),
    "/settings/email": () => import("./routes/AdminEmail.svelte"),
    "/settings/upload": () => import("./routes/AdminUpload.svelte"),
    "/settings/team": () => import("./routes/TeamSettings.svelte"),
    "/settings/team-users": () => import("./routes/TeamUsers.svelte"),
  };
  const notFoundLoader = () => import("./routes/NotFound.svelte");

  const routeAccess = {
    "/time": (user) => user?.tracks_time !== false,
    "/absences": (user) => user?.tracks_time !== false,
    "/calendar": (user) =>
      user?.tracks_time !== false || !!user?.permissions?.can_view_team_reports,
    "/dashboard": (user) => !!user?.permissions?.can_view_dashboard,
    "/reports": (user) => !!user?.permissions?.can_view_reports,
    "/settings/general": (user) => !!user?.permissions?.can_manage_settings,
    "/settings/users": (user) => !!user?.permissions?.can_manage_users,
    "/settings/archived-users": (user) => !!user?.permissions?.can_manage_users,
    "/settings/categories": (user) => !!user?.permissions?.can_manage_categories,
    "/settings/holidays": (user) => !!user?.permissions?.can_manage_holidays,
    "/settings/audit-log": (user) => !!user?.permissions?.can_view_audit_log,
    "/settings/email": (user) => !!user?.permissions?.can_manage_settings,
    "/settings/upload": (user) => !!user?.permissions?.can_manage_settings,
    "/settings/team": (user) => !!user?.permissions?.can_manage_team_settings,
    "/settings/team-users": (user) => !!user?.permissions?.can_manage_team_users,
  };

  $: routePromise = resolveRoute(pathname, $currentUser);
  $: document.title = $settings?.organization_name
    ? `${$t("Time tracking")} - ${$settings.organization_name}`
    : $t("Time tracking");
  // Show the settings tab bar whenever the user is in the /settings/* area and
  // has at least the team-settings permission (covers both admins and leads).
  $: isSettings =
    pathname.startsWith("/settings") &&
    !!(
      $currentUser?.permissions?.can_manage_settings ||
      $currentUser?.permissions?.can_manage_team_settings
    );

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

  function componentFromModule(loader) {
    return loader().then((module) => module.default);
  }

  function loadRoute(path) {
    return componentFromModule(routeLoaders[path] || notFoundLoader);
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
    if (p === "/" || p === "" || p === "/settings") {
      // "/settings" (no sub-path) redirects to the first accessible settings tab.
      // Team leads with assistant management access land on the Users tab;
      // other leads land on the team tab; admins on general settings.
      const settingsDest = user?.permissions?.can_manage_settings
        ? "/settings/general"
        : user?.permissions?.can_manage_team_users
          ? "/settings/team-users"
          : "/settings/team";
      const dest = user.must_change_password
        ? "/account"
        : user.must_configure_settings
          ? "/settings/general"
          : p === "/settings"
            ? settingsDest
            : preferredHome(user);
      debugLog("route:redirect-home", { dest });
      // Update the URL bar (deferred so we don't mutate stores mid-reactive-cycle)
      setTimeout(() => go(dest, false), 0);
      return loadRoute(dest);
    }
    if (user.must_change_password && p !== "/account") {
      debugLog("route:redirect-password-change");
      setTimeout(() => go("/account", false), 0);
      return loadRoute("/account");
    }
    // Only redirect to settings setup when the password is already in order,
    // so an admin with both flags can complete the password change first.
    if (
      user.must_configure_settings &&
      !user.must_change_password &&
      p !== "/settings/general"
    ) {
      debugLog("route:redirect-configure-settings");
      setTimeout(() => go("/settings/general", false), 0);
      return loadRoute("/settings/general");
    }
    if (routeLoaders[p] && !canAccessRoute(p, user)) {
      const dest = preferredHome(user);
      debugLog("route:redirect-unauthorized", {
        inputPath: p,
        dest,
      });
      setTimeout(() => go(dest, false), 0);
      return loadRoute(dest);
    }
    const routeExists = !!routeLoaders[p];
    debugLog("route:resolved", {
      inputPath: p,
      resolved: routeExists ? p : "not-found",
    });
    return loadRoute(p);
  }

  // Intercept data-link clicks
  function onClick(event) {
    const linkElement = event.target.closest("a[data-link]");
    if (linkElement) {
      event.preventDefault();
      go(linkElement.getAttribute("href"));
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
      class="zf-btn zf-btn-primary"
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
{:else if needsSetup}
  <Setup
    onComplete={(email) => {
      setupEmail = email;
      needsSetup = false;
    }}
  />
{:else if !$currentUser}
  <Login initialEmail={setupEmail} />
{:else if routePromise}
  <Layout>
    {#if isSettings}
      <SettingsTabs />
    {/if}
    {#key pathname}
      {#await routePromise}
        <p style="padding: 2em">{$t("Loading...")}</p>
      {:then route}
        <svelte:component this={route} />
      {/await}
    {/key}
  </Layout>
{:else}
  <p style="padding: 2em">{$t("Loading...")}</p>
{/if}

<div class="toast-container">
  {#each $toasts as item (item.id)}
    <div class="toast toast-{item.type}">{item.message}</div>
  {/each}
</div>
