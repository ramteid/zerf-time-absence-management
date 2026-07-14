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
  let tracksTime = true;
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
          tracks_time: tracksTime,
        },
      });
      await storePasswordCredential(form);
      // Notify the parent (App.svelte) so it can transition to the login
      // screen and pre-fill the email without a full page reload.
      onComplete(email.trim());
    } catch (err) {
      error = $t(err?.message || "Error");
    } finally {
      submitting = false;
    }
  }
</script>

<div class="login-wrap">
  <div class="zf-card login-card">
    <div class="login-logo">
      <div class="login-logo-icon">
        <Icon name="Clock" size={18} />
      </div>
      <h1 class="auth-title">
        ZERF {$t("Time tracking")}
      </h1>
    </div>
    <p class="zf-form-intro">
      {$t("Create the initial administrator account to get started.")}
    </p>
    <form
      name="setup"
      method="post"
      action="/api/v1/auth/setup"
      autocomplete="on"
      on:submit={submit}
    >
      <div class="name-row">
        <div class="flex-1">
          <label class="zf-label" for="setup-first-name"
            >{$t("First name")}</label
          >
          <input
            id="setup-first-name"
            class="zf-input"
            type="text"
            bind:value={firstName}
            required
            maxlength="200"
            autocomplete="given-name"
          />
        </div>
        <div class="flex-1">
          <label class="zf-label" for="setup-last-name">{$t("Last name")}</label
          >
          <input
            id="setup-last-name"
            class="zf-input"
            type="text"
            bind:value={lastName}
            required
            maxlength="200"
            autocomplete="family-name"
          />
        </div>
      </div>
      <div class="mb-14">
        <label class="zf-label" for="setup-email">{$t("Email")}</label>
        <input
          id="setup-email"
          name="username"
          class="zf-input"
          type="email"
          bind:value={email}
          required
          autocomplete="username"
        />
      </div>
      <div class="mb-14">
        <label class="zf-label" for="setup-password">{$t("Password")}</label>
        <input
          id="setup-password"
          name="password"
          class="zf-input"
          type="password"
          bind:value={password}
          required
          minlength="12"
          autocomplete="new-password"
        />
      </div>
      <div class="mb-14">
        <label class="zf-label" for="setup-confirm"
          >{$t("Confirm password")}</label
        >
        <input
          id="setup-confirm"
          name="password_confirm"
          class="zf-input"
          type="password"
          bind:value={confirmPassword}
          required
          minlength="12"
          autocomplete="new-password"
        />
      </div>
      <label class="opt-check">
        <input type="checkbox" bind:checked={tracksTime} />
        <div>
          <div class="opt-title">
            {$t("Enable time tracking for this account")}
          </div>
          <div class="opt-desc">
            {$t(
              "When disabled, this admin works in management-only mode (no time entries or absences).",
            )}
          </div>
        </div>
      </label>
      <div class="error-text mb-8">{error}</div>
      <button
        class="zf-btn zf-btn-primary zf-btn-block"
        type="submit"
        disabled={submitting}
      >
        {submitting ? $t("Creating account…") : $t("Create admin account")}
      </button>
    </form>
  </div>
</div>

<style>
  .name-row {
    display: flex;
    gap: 10px;
    margin-bottom: 14px;
  }

  /* Opt-in checkbox with a title and description; checkbox aligns with the
     first text line. */
  .opt-check {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    margin-bottom: 14px;
    cursor: pointer;
  }

  .opt-check input {
    margin-top: 2px;
  }

  .opt-title {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
  }

  .opt-desc {
    font-size: 12px;
    color: var(--text-tertiary);
    margin-top: 2px;
  }
</style>
