<script>
  import { api, csrfToken, resetUnauthorizedGate } from "../api.js";
  import { currentUser, categories, go } from "../stores.js";
  import { t } from "../i18n.js";
  import Icon from "../Icons.svelte";

  let email = "";
  let password = "";
  let error = "";
  let submitting = false;

  async function submit(e) {
    e.preventDefault();
    if (submitting) return;
    error = "";
    submitting = true;
    try {
      const r = await api("/auth/login", {
        method: "POST",
        body: { email, password },
      });
      // Always take the CSRF token from the login response first...
      csrfToken.set(r.csrf_token || null);
      const me = await api("/auth/me");
      // ...then overwrite with the one from /me (authoritative, fresher).
      csrfToken.set(me.csrf_token || null);
      // Set the path BEFORE currentUser so that when the reactive chain fires
      // in App.svelte, matchRoute already sees the correct pathname instead of
      // "/" — which would return null and flash "Wird geladen...".
      const dest = me.must_change_password ? "/account" : me.home || "/time";
      go(dest);
      currentUser.set(me);
      // Re-arm the session-expiry gates only after login is fully committed.
      resetUnauthorizedGate();
      try {
        categories.set(await api("/categories"));
      } catch {}
    } catch (err) {
      error = err.message || "Error";
    } finally {
      submitting = false;
    }
  }
</script>

<div class="login-wrap">
  <div class="kz-card login-card">
    <div class="login-logo">
      <div class="login-logo-icon">
        <Icon name="Clock" size={18} />
      </div>
      <h1
        style="margin:0;font-size:20px;font-weight:600;letter-spacing:-0.02em"
      >
        KitaZeit
      </h1>
    </div>
    <p style="font-size:13px;color:var(--text-tertiary);margin-bottom:24px">
      {$t("Sign in to your time-tracking workspace.")}
    </p>
    <form on:submit={submit}>
      <div style="margin-bottom:14px">
        <label class="kz-label" for="email">{$t("Email")}</label>
        <input
          id="email"
          class="kz-input"
          type="email"
          bind:value={email}
          required
          autocomplete="email"
        />
      </div>
      <div style="margin-bottom:14px">
        <label class="kz-label" for="password">{$t("Password")}</label>
        <input
          id="password"
          class="kz-input"
          type="password"
          bind:value={password}
          required
          autocomplete="current-password"
        />
      </div>
      <div class="error-text" style="margin-bottom:8px">{error}</div>
      <button
        class="kz-btn kz-btn-primary"
        type="submit"
        disabled={submitting}
        style="width:100%;justify-content:center;height:38px"
      >
        {submitting ? $t("Signing in…") : $t("Sign in")}
      </button>
    </form>
  </div>
</div>
