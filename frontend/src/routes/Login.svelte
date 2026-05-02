<script>
  import { api, csrfToken } from "../api.js";
  import { currentUser, categories, go } from "../stores.js";
  import { t } from "../i18n.js";
  import Icon from "../Icons.svelte";

  let email = "";
  let password = "";
  let error = "";

  async function submit(e) {
    e.preventDefault();
    error = "";
    try {
      const r = await api("/auth/login", {
        method: "POST",
        body: { email, password },
      });
      csrfToken.set(r.csrf_token || null);
      const me = await api("/auth/me");
      currentUser.set(me);
      try {
        categories.set(await api("/categories"));
      } catch {}
      go(me.must_change_password ? "/account" : me.home || "/time");
    } catch (err) {
      error = err.message || "Error";
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
        style="width:100%;justify-content:center;height:38px"
      >
        {$t("Sign in")}
      </button>
    </form>
  </div>
</div>
