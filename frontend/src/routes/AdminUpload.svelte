<script>
  import { onDestroy } from "svelte";
  import { api } from "../api.js";
  import { toast } from "../stores.js";
  import { t } from "../i18n.js";
  import { fmtDateTime } from "../format.js";

  let uploadSettings = {};
  let saving = false;
  let uploading = false;
  let requestingBackup = false;
  let backupPollTimer = null;

  // Passwords are write-only; we track new values separately.
  let backupUploadPassword = "";
  let clearBackupPassword = false;
  let reportUploadPassword = "";
  let clearReportPassword = false;

  async function load() {
    uploadSettings = await api("/settings");
  }
  load();

  // The more recent of the two backup timestamps -- a scheduled run
  // (backup_last_success_at) and a manual "Back up now" run
  // (backup_last_manual_at) are tracked separately on the backend (see
  // AGENTS.md) so a manual click never postpones the schedule, but the admin
  // just wants to know when a backup last actually happened.
  $: lastBackupAt = laterOf(
    uploadSettings.backup_last_success_at,
    uploadSettings.backup_last_manual_at,
  );

  function laterOf(a, b) {
    if (!a) return b || null;
    if (!b) return a;
    // Both are UTC timestamps in the same zero-padded ISO 8601 shape written
    // by backup.sh (YYYY-MM-DDTHH:MM:SSZ), so lexicographic string comparison
    // is also chronological order -- no need to parse them into Dates.
    return a > b ? a : b;
  }

  function stopBackupPoll() {
    if (backupPollTimer) {
      clearInterval(backupPollTimer);
      backupPollTimer = null;
    }
  }
  onDestroy(stopBackupPoll);

  // The backup itself happens in a separate, network-isolated container
  // (see AGENTS.md's Deployment section, `backup_net`) that polls for the
  // request rather than being called directly, so it is not done by the time
  // this request returns. Poll briefly afterward so "Last backup" updates on
  // its own once it lands, instead of requiring a manual page reload.
  function pollForBackupCompletion(previousLastBackup) {
    stopBackupPoll();
    let attempts = 0;
    backupPollTimer = setInterval(async () => {
      attempts += 1;
      await load();
      if (lastBackupAt !== previousLastBackup || attempts >= 6) {
        stopBackupPoll();
      }
    }, 10000);
  }

  function backupPasswordPayload() {
    if (clearBackupPassword) return "";
    return backupUploadPassword || undefined;
  }

  function reportPasswordPayload() {
    if (clearReportPassword) return "";
    return reportUploadPassword || undefined;
  }

  async function save() {
    if (
      uploadSettings.backup_upload_enabled &&
      !(uploadSettings.backup_upload_url || "").trim()
    ) {
      toast(
        $t(
          "A Nextcloud share URL is required to enable database backup upload.",
        ),
        "error",
      );
      return;
    }
    saving = true;
    try {
      const body = {
        backup_upload_enabled: !!uploadSettings.backup_upload_enabled,
        backup_upload_url: uploadSettings.backup_upload_url || "",
        backup_upload_password: backupPasswordPayload(),
        backup_interval_days:
          parseInt(uploadSettings.backup_interval_days) || 1,
        report_upload_enabled: !!uploadSettings.report_upload_enabled,
        report_upload_url: uploadSettings.report_upload_url || "",
        report_upload_password: reportPasswordPayload(),
        report_upload_day_of_month:
          parseInt(uploadSettings.report_upload_day_of_month) || 5,
      };
      const saved = await api("/settings/uploads", { method: "PUT", body });
      uploadSettings = saved;
      backupUploadPassword = "";
      clearBackupPassword = false;
      reportUploadPassword = "";
      clearReportPassword = false;
      toast($t("Settings saved."), "ok");
    } catch (e) {
      toast(e?.message || $t("Error"), "error");
    } finally {
      saving = false;
    }
  }

  async function runNow() {
    uploading = true;
    try {
      await api("/settings/uploads/report/run-now", { method: "POST" });
      toast($t("Upload started."), "ok");
    } catch (e) {
      toast(e?.message || $t("Upload failed."), "error");
    } finally {
      uploading = false;
    }
  }

  async function backupNow() {
    requestingBackup = true;
    const previousLastBackup = lastBackupAt;
    try {
      await api("/settings/uploads/backup/run-now", { method: "POST" });
      toast($t("Backup requested."), "ok");
      pollForBackupCompletion(previousLastBackup);
    } catch (e) {
      toast(e?.message || $t("Backup request failed."), "error");
    } finally {
      requestingBackup = false;
    }
  }
</script>

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t("Nextcloud Backups")}</h1>
  </div>
</div>

<div class="content-area">
  <!-- Database backups -->
  <div class="zf-card zf-card-section">
    <div class="field-card-title">{$t("Database backups")}</div>
    <div class="field-group">
      <div class="field-row">
        <div>
          <label class="zf-label zf-row">
            <input
              type="checkbox"
              bind:checked={uploadSettings.backup_upload_enabled}
            />
            {$t("Upload database backups")}
          </label>
        </div>
      </div>

      <div class="field-row">
        <div>
          <label class="zf-label" for="backup-upload-url"
            >{$t("Nextcloud share link")}</label
          >
          <input
            id="backup-upload-url"
            class="zf-input"
            type="url"
            bind:value={uploadSettings.backup_upload_url}
            placeholder="https://nextcloud.example.com/s/abc123"
            disabled={!uploadSettings.backup_upload_enabled}
          />
        </div>
        <div>
          <label class="zf-label" for="backup-upload-password">
            {$t("Password (optional)")}
            {#if uploadSettings.backup_upload_password_set}
              <span class="zf-label-hint">({$t("stored")})</span>
            {/if}
          </label>
          <input
            id="backup-upload-password"
            class="zf-input"
            type="password"
            bind:value={backupUploadPassword}
            on:input={() => (clearBackupPassword = false)}
            placeholder={uploadSettings.backup_upload_password_set
              ? "********"
              : ""}
            autocomplete="new-password"
            disabled={!uploadSettings.backup_upload_enabled}
          />
          {#if uploadSettings.backup_upload_password_set}
            <label class="zf-label zf-row mt-8">
              <input
                type="checkbox"
                bind:checked={clearBackupPassword}
                disabled={!!backupUploadPassword}
              />
              {$t("Clear stored password")}
            </label>
          {/if}
        </div>
      </div>

      <div class="field-row">
        <div>
          <label class="zf-label" for="backup-interval"
            >{$t("Days between backups")}</label
          >
          <input
            id="backup-interval"
            class="zf-input"
            type="number"
            min="1"
            bind:value={uploadSettings.backup_interval_days}
            placeholder="1"
          />
          <div class="field-hint">
            {$t(
              "The 10 latest backups stay on this server; older ones are deleted. Files in Nextcloud are not deleted automatically.",
            )}
          </div>
        </div>
      </div>

      <div class="field-row">
        <div>
          <button
            class="zf-btn zf-btn-accent-soft"
            on:click={backupNow}
            disabled={requestingBackup || saving}
          >
            {#if requestingBackup}
              {$t("Requesting...")}
            {:else}
              {$t("Back up now")}
            {/if}
          </button>
          <div class="field-hint">
            {#if lastBackupAt}
              {$t("Last backup: {time}", { time: fmtDateTime(lastBackupAt) })}
            {:else}
              {$t("No backup has run yet.")}
            {/if}
            {$t(
              "The backup runs in the background and usually starts within a few seconds.",
            )}
          </div>
        </div>
      </div>
    </div>
  </div>

  <!-- Timesheets -->
  <div class="zf-card zf-card-section">
    <div class="field-card-title">{$t("Timesheets")}</div>
    <div class="field-group">
      <div class="field-row">
        <div>
          <label class="zf-label zf-row">
            <input
              type="checkbox"
              bind:checked={uploadSettings.report_upload_enabled}
            />
            {$t("Upload timesheets")}
          </label>
          <div class="field-hint">
            {$t(
              "Uploads the previous month's timesheets on the selected day. If submissions or approvals are missing, the upload happens later automatically.",
            )}
          </div>
        </div>
      </div>

      <div class="field-row">
        <div>
          <label class="zf-label" for="report-upload-url"
            >{$t("Nextcloud share link")}</label
          >
          <input
            id="report-upload-url"
            class="zf-input"
            type="url"
            bind:value={uploadSettings.report_upload_url}
            placeholder="https://nextcloud.example.com/s/xyz456"
            disabled={!uploadSettings.report_upload_enabled}
          />
        </div>
        <div>
          <label class="zf-label" for="report-upload-password">
            {$t("Password (optional)")}
            {#if uploadSettings.report_upload_password_set}
              <span class="zf-label-hint">({$t("stored")})</span>
            {/if}
          </label>
          <input
            id="report-upload-password"
            class="zf-input"
            type="password"
            bind:value={reportUploadPassword}
            on:input={() => (clearReportPassword = false)}
            placeholder={uploadSettings.report_upload_password_set
              ? "********"
              : ""}
            autocomplete="new-password"
            disabled={!uploadSettings.report_upload_enabled}
          />
          {#if uploadSettings.report_upload_password_set}
            <label class="zf-label zf-row mt-8">
              <input
                type="checkbox"
                bind:checked={clearReportPassword}
                disabled={!!reportUploadPassword}
              />
              {$t("Clear stored password")}
            </label>
          {/if}
        </div>
      </div>

      <div class="field-row">
        <div>
          <label class="zf-label" for="report-upload-day"
            >{$t("Upload day (1-28)")}</label
          >
          <input
            id="report-upload-day"
            class="zf-input"
            type="number"
            min="1"
            max="28"
            bind:value={uploadSettings.report_upload_day_of_month}
            placeholder="5"
          />
        </div>
      </div>

      <div class="field-row">
        <div>
          <button
            class="zf-btn zf-btn-accent-soft"
            on:click={runNow}
            disabled={uploading ||
              saving ||
              !uploadSettings.report_upload_enabled}
          >
            {#if uploading}
              {$t("Uploading...")}
            {:else}
              {$t("Upload now")}
            {/if}
          </button>
        </div>
      </div>
    </div>
  </div>

  <!-- Actions -->
  <div class="zf-card zf-card-section">
    <div class="form-actions">
      <button
        class="zf-btn zf-btn-primary"
        on:click={save}
        disabled={saving || uploading}
      >
        {#if saving}
          {$t("Saving...")}
        {:else}
          {$t("Save")}
        {/if}
      </button>
    </div>
  </div>
</div>

<style>
  /* Matches the Payroll Report page's action bar (gap, padding-top). */
  .form-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding-top: 16px;
  }
</style>
