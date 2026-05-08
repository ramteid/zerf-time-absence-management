<script>
  import { onMount, onDestroy } from "svelte";
  import { api, csrfToken } from "../api.js";
  import { settings } from "../stores.js";
  import { t } from "../i18n.js";
  import Icon from "../Icons.svelte";
  import { storePasswordCredential } from "../passwordCredentials.js";

  // On mobile, the virtual keyboard shrinks the visual viewport but not the layout
  // viewport. By removing the fixed height and overflow lock from the root elements,
  // the browser can scroll naturally to keep the focused input above the keyboard.
  onMount(() => {
    document.documentElement.style.height = "auto";
    document.documentElement.style.overflow = "auto";
    document.body.style.height = "auto";
    document.body.style.overflow = "auto";
    document.getElementById("app").style.height = "auto";
    document.getElementById("app").style.overflow = "visible";

    // If a reset token is present in the URL, switch directly to reset view.
    const params = new URLSearchParams(window.location.search);
    const token = params.get("reset_token");
    if (token) {
      resetToken = token;
      view = "reset";
      // Clean the token out of the address bar without adding a history entry.
      history.replaceState(null, "", window.location.pathname);
    }
  });
  onDestroy(() => {
    document.documentElement.style.height = "";
    document.documentElement.style.overflow = "";
    document.body.style.height = "";
    document.body.style.overflow = "";
    document.getElementById("app").style.height = "";
    document.getElementById("app").style.overflow = "";
  });

  export let initialEmail = "";

  // "login" | "forgot" | "reset"
  let view = "login";

  // Login — fall back to sessionStorage for the post-setup-reload case
  const _storedEmail = sessionStorage.getItem("setup-email") || "";
  if (_storedEmail) sessionStorage.removeItem("setup-email");
  let email = initialEmail || _storedEmail;
  let password = "";
  let loginError = "";
  let submitting = false;

  async function submitLogin(e) {
    const form = e.currentTarget;
    e.preventDefault();
    if (submitting) return;
    loginError = "";
    submitting = true;
    try {
      const loginResponse = await api("/auth/login", {
        method: "POST",
        body: { email, password },
      });
      csrfToken.set(loginResponse.csrf_token || null);
      const currentUserResponse = await api("/auth/me");
      const dashboardAvailable = (currentUserResponse?.nav || []).some(
        (item) => item?.key === "Dashboard" || item?.href === "/dashboard",
      );
      const dest = currentUserResponse.must_change_password
        ? "/account"
        : currentUserResponse.must_configure_settings
          ? "/admin/settings"
          : dashboardAvailable
            ? "/dashboard"
            : currentUserResponse.home || "/time";
      await storePasswordCredential(form);
      window.location.assign(dest);
    } catch (err) {
      if (err?.apiMessage === "account_deactivated") {
        loginError = $t("account_deactivated");
      } else {
        loginError = $t(err?.message || "Error");
      }
    } finally {
      submitting = false;
    }
  }

  // Forgot password
  let forgotEmail = "";
  let forgotError = "";
  let forgotSent = false;
  let forgotBusy = false;

  async function submitForgot(e) {
    e.preventDefault();
    if (forgotBusy) return;
    forgotError = "";
    forgotBusy = true;
    try {
      await api("/auth/forgot-password", {
        method: "POST",
        body: { email: forgotEmail },
      });
      forgotSent = true;
    } catch (err) {
      if (err?.apiMessage === "password_reset_unavailable") {
        forgotError = $t("password_reset_unavailable");
      } else {
        forgotError = $t(err?.message || "Error");
      }
    } finally {
      forgotBusy = false;
    }
  }

  // Reset password
  let resetToken = "";
  let newPassword = "";
  let newPassword2 = "";
  let resetError = "";
  let resetDone = false;
  let resetBusy = false;

  async function submitReset(e) {
    e.preventDefault();
    if (resetBusy) return;
    resetError = "";
    if (newPassword !== newPassword2) {
      resetError = $t("Passwords do not match.");
      return;
    }
    resetBusy = true;
    try {
      await api("/auth/reset-password", {
        method: "POST",
        body: { token: resetToken, password: newPassword },
      });
      resetDone = true;
    } catch (err) {
      if (err?.apiMessage === "reset_token_expired") {
        resetError = $t("reset_token_expired");
      } else if (err?.apiMessage === "reset_token_invalid") {
        resetError = $t("reset_token_invalid");
      } else {
        resetError = $t(err?.message || "Error");
      }
    } finally {
      resetBusy = false;
    }
  }
</script>

<div class="login-wrap">
  <div class="kz-card login-card">
    <div class="login-logo">
      <div class="login-logo-icon">
        <Icon name="Clock" size={18} />
      </div>
      <div>
        <h1
          style="margin:0;font-size:20px;font-weight:400;letter-spacing:-0.02em"
        >
          ZERF {$t("Time tracking")}
        </h1>
        {#if $settings?.organization_name}
          <div style="font-size:12px;color:var(--text-tertiary);margin-top:2px">
            {$settings.organization_name}
          </div>
        {/if}
      </div>
    </div>

    {#if view === "login"}
      <p style="font-size:13px;color:var(--text-tertiary);margin-bottom:24px">
        {$t("Sign in to your time-tracking workspace.")}
      </p>
      <form
        name="login"
        method="post"
        action="/api/v1/auth/login"
        autocomplete="on"
        on:submit={submitLogin}
      >
        <div style="margin-bottom:14px">
          <label class="kz-label" for="email">{$t("Email")}</label>
          <input
            id="email"
            name="username"
            class="kz-input"
            type="email"
            bind:value={email}
            required
            autocomplete="username"
          />
        </div>
        <div style="margin-bottom:14px">
          <label class="kz-label" for="password">{$t("Password")}</label>
          <input
            id="password"
            name="password"
            class="kz-input"
            type="password"
            bind:value={password}
            required
            autocomplete="current-password"
          />
        </div>
        <div class="error-text" style="margin-bottom:8px">{loginError}</div>
        <button
          class="kz-btn kz-btn-primary"
          type="submit"
          disabled={submitting}
          style="width:100%;justify-content:center;height:38px"
        >
          {submitting ? $t("Signing in…") : $t("Sign in")}
        </button>
      </form>
      <div style="text-align:center;margin-top:14px">
        <button
          class="kz-btn kz-btn-ghost kz-btn-sm"
          type="button"
          on:click={() => {
            view = "forgot";
            forgotEmail = email;
            forgotError = "";
            forgotSent = false;
          }}
        >
          {$t("Forgot password?")}
        </button>
      </div>
    {:else if view === "forgot"}
      <p style="font-size:13px;color:var(--text-tertiary);margin-bottom:24px">
        {$t("Enter your email to receive a password reset link.")}
      </p>
      {#if forgotSent}
        <div
          style="font-size:13px;color:var(--success-text);background:var(--success-soft);padding:12px;border-radius:var(--radius-sm);margin-bottom:16px"
        >
          {$t(
            "If your email address is registered, you will receive a reset link shortly.",
          )}
        </div>
        <button
          class="kz-btn kz-btn-ghost kz-btn-sm"
          style="width:100%;justify-content:center"
          on:click={() => (view = "login")}
        >
          {$t("Back to sign in")}
        </button>
      {:else}
        <form on:submit={submitForgot}>
          <div style="margin-bottom:14px">
            <label class="kz-label" for="forgot-email">{$t("Email")}</label>
            <input
              id="forgot-email"
              class="kz-input"
              type="email"
              bind:value={forgotEmail}
              required
              autocomplete="email"
            />
          </div>
          <div class="error-text" style="margin-bottom:8px">{forgotError}</div>
          <button
            class="kz-btn kz-btn-primary"
            type="submit"
            disabled={forgotBusy}
            style="width:100%;justify-content:center;height:38px"
          >
            {forgotBusy ? $t("Sending...") : $t("Send reset link")}
          </button>
        </form>
        <div style="text-align:center;margin-top:14px">
          <button
            class="kz-btn kz-btn-ghost kz-btn-sm"
            type="button"
            on:click={() => (view = "login")}
          >
            {$t("Back to sign in")}
          </button>
        </div>
      {/if}
    {:else if view === "reset"}
      <p style="font-size:13px;color:var(--text-tertiary);margin-bottom:24px">
        {$t("Choose a new password for your account.")}
      </p>
      {#if resetDone}
        <div
          style="font-size:13px;color:var(--success-text);background:var(--success-soft);padding:12px;border-radius:var(--radius-sm);margin-bottom:16px"
        >
          {$t("Password reset successfully. Please sign in.")}
        </div>
        <button
          class="kz-btn kz-btn-primary"
          style="width:100%;justify-content:center;height:38px"
          on:click={() => {
            view = "login";
            newPassword = "";
            newPassword2 = "";
          }}
        >
          {$t("Sign in")}
        </button>
      {:else}
        <form on:submit={submitReset}>
          <div style="margin-bottom:14px">
            <label class="kz-label" for="new-password"
              >{$t("New password")}</label
            >
            <input
              id="new-password"
              class="kz-input"
              type="password"
              bind:value={newPassword}
              required
              minlength="12"
              autocomplete="new-password"
            />
          </div>
          <div style="margin-bottom:14px">
            <label class="kz-label" for="new-password2"
              >{$t("Confirm password")}</label
            >
            <input
              id="new-password2"
              class="kz-input"
              type="password"
              bind:value={newPassword2}
              required
              minlength="12"
              autocomplete="new-password"
            />
          </div>
          <div class="error-text" style="margin-bottom:8px">{resetError}</div>
          <button
            class="kz-btn kz-btn-primary"
            type="submit"
            disabled={resetBusy}
            style="width:100%;justify-content:center;height:38px"
          >
            {resetBusy ? $t("Saving...") : $t("Set new password")}
          </button>
        </form>
        <div style="text-align:center;margin-top:14px">
          <button
            class="kz-btn kz-btn-ghost kz-btn-sm"
            type="button"
            on:click={() => (view = "login")}
          >
            {$t("Back to sign in")}
          </button>
        </div>
      {/if}
    {/if}
  </div>
</div>
