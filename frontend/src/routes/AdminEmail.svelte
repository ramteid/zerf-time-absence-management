<script>
  import { api } from "../api.js";
  import { toast } from "../stores.js";
  import { t } from "../i18n.js";

  let smtpSettings = {};
  let saving = false;
  let smtpPassword = "";
  let testing = false;
  let testResult = null;

  async function load() {
    smtpSettings = await api("/settings");
    if (smtpSettings.smtp_enabled) {
      testConnection(true);
    }
  }
  load();

  async function save() {
    saving = true;
    try {
      const body = {
        smtp_enabled: !!smtpSettings.smtp_enabled,
        smtp_host: smtpSettings.smtp_host || "",
        smtp_port: parseInt(smtpSettings.smtp_port) || 587,
        smtp_username: smtpSettings.smtp_username || "",
        smtp_password: smtpPassword || undefined,
        smtp_from: smtpSettings.smtp_from || "",
        smtp_encryption: smtpSettings.smtp_encryption || "starttls",
        submission_reminders_enabled: smtpSettings.submission_reminders_enabled !== false,
      };
      const saved = await api("/settings/smtp", { method: "PUT", body });
      Object.assign(smtpSettings, saved);
      smtpPassword = "";
      toast($t("SMTP settings saved."), "ok");
      if (body.smtp_enabled) {
        testConnection(true);
      } else {
        testResult = null;
      }
    } catch (e) {
      testResult = { ok: false, message: e?.message || $t("Error") };
      toast(e?.message || $t("Error"), "error");
    } finally {
      saving = false;
    }
  }

  async function testConnection(silent = false) {
    testing = true;
    testResult = null;
    try {
      const body = {
        smtp_enabled: true,
        smtp_host: smtpSettings.smtp_host || "",
        smtp_port: parseInt(smtpSettings.smtp_port) || 587,
        smtp_username: smtpSettings.smtp_username || "",
        smtp_password: smtpPassword || undefined,
        smtp_from: smtpSettings.smtp_from || "",
        smtp_encryption: smtpSettings.smtp_encryption || "starttls",
      };
      await api("/settings/smtp/test", { method: "POST", body });
      testResult = { ok: true };
      if (!silent) toast($t("SMTP connection successful."), "ok");
    } catch (e) {
      testResult = { ok: false, message: e?.message || $t("Error") };
      if (!silent) toast(e?.message || $t("Error"), "error");
    } finally {
      testing = false;
    }
  }
</script>

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t("Email (SMTP)")}</h1>
  </div>
</div>

<div class="content-area">
  <div class="kz-card" style="padding:20px;margin-bottom:16px">
    <div style="display:flex;align-items:center;gap:10px;margin-bottom:16px">
      <span
        class="smtp-status-dot"
        style="width:10px;height:10px;border-radius:50%;flex-shrink:0;background:{testResult ? (testResult.ok ? 'var(--success, #22c55e)' : 'var(--error, #ef4444)') : 'var(--text-tertiary, #888)'}"
      ></span>
      <span style="font-size:13px;font-weight:500;color:var(--text-secondary)">
        {#if testResult}
          {testResult.ok ? $t("Connection OK") : testResult.message}
        {:else}
          {$t("Not tested")}
        {/if}
      </span>
    </div>
    <div class="field-group">
      <div class="field-row">
        <div>
          <label class="kz-label" style="display:flex;align-items:center;gap:8px">
            <input
              type="checkbox"
              bind:checked={smtpSettings.smtp_enabled}
              style="width:auto"
            />
            {$t("Enable SMTP")}
          </label>
          <div style="font-size:11px;color:var(--text-tertiary);margin-top:4px">
            {$t("When enabled, notification emails are sent for approvals, rejections, and reopen requests.")}
          </div>
        </div>
      </div>

      <div class="field-row" style="margin-top:8px">
        <div>
          <label class="kz-label" style="display:flex;align-items:center;gap:8px">
            <input
              type="checkbox"
              bind:checked={smtpSettings.submission_reminders_enabled}
              style="width:auto"
              disabled={!smtpSettings.smtp_enabled}
            />
            {$t("Enable reminders")}
          </label>
          <div style="font-size:11px;color:var(--text-tertiary);margin-top:4px">
            {$t("When enabled, users who have not submitted all time entries are reminded by email on the configured deadline day.")}
          </div>
        </div>
      </div>

      <div class="field-row" style="margin-top:12px">
        <div>
          <label class="kz-label" for="smtp-host">{$t("SMTP Host")}</label>
          <input
            id="smtp-host"
            class="kz-input"
            bind:value={smtpSettings.smtp_host}
            placeholder="smtp.example.com"
          />
        </div>
        <div>
          <label class="kz-label" for="smtp-port">{$t("SMTP Port")}</label>
          <input
            id="smtp-port"
            class="kz-input"
            type="number"
            bind:value={smtpSettings.smtp_port}
            placeholder="587"
          />
        </div>
      </div>

      <div class="field-row" style="margin-top:12px">
        <div>
          <label class="kz-label" for="smtp-username">{$t("Username")}</label>
          <input
            id="smtp-username"
            class="kz-input"
            bind:value={smtpSettings.smtp_username}
            autocomplete="off"
          />
        </div>
        <div>
          <label class="kz-label" for="smtp-password">
            {$t("Password")}
            {#if smtpSettings.smtp_password_set}
              <span style="font-size:11px;color:var(--text-tertiary);font-weight:normal">({$t("stored")})</span>
            {/if}
          </label>
          <input
            id="smtp-password"
            class="kz-input"
            type="password"
            bind:value={smtpPassword}
            placeholder={smtpSettings.smtp_password_set ? "********" : ""}
            autocomplete="new-password"
          />
        </div>
      </div>

      <div class="field-row" style="margin-top:12px">
        <div>
          <label class="kz-label" for="smtp-from">{$t("From address")}</label>
          <input
            id="smtp-from"
            class="kz-input"
            bind:value={smtpSettings.smtp_from}
            placeholder='Zerf <noreply@example.com>'
          />
        </div>
        <div>
          <label class="kz-label" for="smtp-encryption">{$t("Encryption")}</label>
          <select
            id="smtp-encryption"
            class="kz-select"
            bind:value={smtpSettings.smtp_encryption}
          >
            <option value="starttls">STARTTLS</option>
            <option value="tls">TLS</option>
            <option value="none">{$t("None")}</option>
          </select>
        </div>
      </div>

      <div style="display:flex;justify-content:flex-end;gap:8px;padding-top:16px">
        <button class="kz-btn" on:click={testConnection} disabled={testing || saving}>
          {#if testing}
            {$t("Testing...")}
          {:else}
            {$t("Test Connection")}
          {/if}
        </button>
        <button class="kz-btn kz-btn-primary" on:click={save} disabled={saving || testing}>
          {#if saving}
            {$t("Saving...")}
          {:else}
            {$t("Save")}
          {/if}
        </button>
      </div>
    </div>
  </div>
</div>
