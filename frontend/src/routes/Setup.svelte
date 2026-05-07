<script>
  import { onMount, onDestroy } from "svelte";
  import { api } from "../api.js";
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
  });
  onDestroy(() => {
    document.documentElement.style.height = "";
    document.documentElement.style.overflow = "";
    document.body.style.height = "";
    document.body.style.overflow = "";
    document.getElementById("app").style.height = "";
    document.getElementById("app").style.overflow = "";
  });

  export let onComplete = () => {};

  let email = "";
  let password = "";
  let confirmPassword = "";
  let firstName = "";
  let lastName = "";
  let error = "";
  let submitting = false;

  async function submit(e) {
    const form = e.currentTarget;
    e.preventDefault();
    if (submitting) return;
    error = "";

    if (!firstName.trim() || !lastName.trim()) {
      error = $t("Please enter your first name and last name.");
      return;
    }
    if (!email.trim() || !email.includes("@")) {
      error = $t("Please enter a valid email address.");
      return;
    }
    if (password.length < 12) {
      error = $t("Password must be at least 12 characters.");
      return;
    }
    const hasLower = /[a-z]/.test(password);
    const hasUpper = /[A-Z]/.test(password);
    const hasDigit = /\d/.test(password);
    const hasSymbol = /[^a-zA-Z0-9]/.test(password);
    if ([hasLower, hasUpper, hasDigit, hasSymbol].filter(Boolean).length < 3) {
      error = $t(
        "Password must include at least 3 of: lowercase, uppercase, digit, symbol.",
      );
      return;
    }
    if (password !== confirmPassword) {
      error = $t("Passwords do not match.");
      return;
    }

    submitting = true;
    try {
      await api("/auth/setup", {
        method: "POST",
        body: {
          email: email.trim(),
          password,
          first_name: firstName.trim(),
          last_name: lastName.trim(),
        },
      });
      await storePasswordCredential(form);
      // A real page navigation lets Firefox and Safari detect the
      // password form submission and offer to save the credentials.
      // Store the email so the login form can pre-fill it after reload.
      sessionStorage.setItem("setup-email", email.trim());
      window.location.reload();
    } catch (err) {
      error = $t(err?.message || "Error");
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
        style="margin:0;font-size:20px;font-weight:400;letter-spacing:-0.02em"
      >
        ZERF {$t("Time tracking")}
      </h1>
    </div>
    <p style="font-size:13px;color:var(--text-tertiary);margin-bottom:24px">
      {$t("Create the initial administrator account to get started.")}
    </p>
    <form
      name="setup"
      method="post"
      action="/api/v1/auth/setup"
      autocomplete="on"
      on:submit={submit}
    >
      <div style="display:flex;gap:10px;margin-bottom:14px">
        <div style="flex:1">
          <label class="kz-label" for="setup-first-name"
            >{$t("First name")}</label
          >
          <input
            id="setup-first-name"
            class="kz-input"
            type="text"
            bind:value={firstName}
            required
            maxlength="200"
            autocomplete="given-name"
          />
        </div>
        <div style="flex:1">
          <label class="kz-label" for="setup-last-name">{$t("Last name")}</label
          >
          <input
            id="setup-last-name"
            class="kz-input"
            type="text"
            bind:value={lastName}
            required
            maxlength="200"
            autocomplete="family-name"
          />
        </div>
      </div>
      <div style="margin-bottom:14px">
        <label class="kz-label" for="setup-email">{$t("Email")}</label>
        <input
          id="setup-email"
          name="username"
          class="kz-input"
          type="email"
          bind:value={email}
          required
          autocomplete="username"
        />
      </div>
      <div style="margin-bottom:14px">
        <label class="kz-label" for="setup-password">{$t("Password")}</label>
        <input
          id="setup-password"
          name="password"
          class="kz-input"
          type="password"
          bind:value={password}
          required
          minlength="12"
          autocomplete="new-password"
        />
      </div>
      <div style="margin-bottom:14px">
        <label class="kz-label" for="setup-confirm"
          >{$t("Confirm password")}</label
        >
        <input
          id="setup-confirm"
          name="password_confirm"
          class="kz-input"
          type="password"
          bind:value={confirmPassword}
          required
          minlength="12"
          autocomplete="new-password"
        />
      </div>
      <div class="error-text" style="margin-bottom:8px">{error}</div>
      <button
        class="kz-btn kz-btn-primary"
        type="submit"
        disabled={submitting}
        style="width:100%;justify-content:center;height:38px"
      >
        {submitting ? $t("Creating account…") : $t("Create admin account")}
      </button>
    </form>
  </div>
</div>
