<script>
  import { api } from "../api.js";
  import { toast } from "../stores.js";
  import { t } from "../i18n.js";

  let uploadSettings = {};
  let saving = false;
  let uploading = false;

  // Passwords are write-only; we track new values separately.
  let backupUploadPassword = "";
  let clearBackupPassword = false;
  let reportUploadPassword = "";
  let clearReportPassword = false;

  async function load() {
    uploadSettings = await api("/settings");
  }
  load();

  function backupPasswordPayload() {
    if (clearBackupPassword) return "";
    return backupUploadPassword || undefined;
  }

  function reportPasswordPayload() {
    if (clearReportPassword) return "";
    return reportUploadPassword || undefined;
  }

  async function save() {
    saving = true;
    try {
      const body = {
        backup_upload_enabled: !!uploadSettings.backup_upload_enabled,
        backup_upload_url: uploadSettings.backup_upload_url || "",
        backup_upload_password: backupPasswordPayload(),
        backup_interval_days:
          parseInt(uploadSettings.backup_interval_days) || 1,
        backup_retention_days:
          parseInt(uploadSettings.backup_retention_days) || 30,
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
      toast($t("Upload settings saved."), "ok");
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
      toast($t("Report uploaded successfully."), "ok");
    } catch (e) {
      toast(e?.message || $t("Upload failed."), "error");
    } finally {
      uploading = false;
    }
  }
</script>

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t("Nextcloud Backups")}</h1>
  </div>
</div>

<div class="content-area">
  <!-- DB Backup Upload -->
  <div class="zf-card" style="padding:20px;margin-bottom:16px">
    <div class="field-card-title">{$t("DB Backup Upload")}</div>
    <div class="field-group">
      <div class="field-row">
        <div>
          <label class="zf-label" style="display:flex;align-items:center;gap:8px">
            <input type="checkbox" bind:checked={uploadSettings.backup_upload_enabled} />
            {$t("Enable DB backup upload")}
          </label>
        </div>
      </div>

      <div class="field-row">
        <div>
          <label class="zf-label" for="backup-upload-url"
            >{$t("Share link (https://…/s/…)")}</label
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
            {$t("Share password (optional)")}
            {#if uploadSettings.backup_upload_password_set}
              <span style="font-size:11px;color:var(--text-tertiary);font-weight:normal"
                >({$t("stored")})</span
              >
            {/if}
          </label>
          <input
            id="backup-upload-password"
            class="zf-input"
            type="password"
            bind:value={backupUploadPassword}
            on:input={() => (clearBackupPassword = false)}
            placeholder={uploadSettings.backup_upload_password_set ? "********" : ""}
            autocomplete="new-password"
            disabled={!uploadSettings.backup_upload_enabled}
          />
          {#if uploadSettings.backup_upload_password_set}
            <label
              class="zf-label"
              style="display:flex;align-items:center;gap:8px;margin-top:8px"
            >
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
            >{$t("Backup interval (days)")}</label
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
              "Backup interval and retention are read by the backup container from the database at the start of each cycle. Changes take effect on the next backup run.",
            )}
          </div>
        </div>
        <div>
          <label class="zf-label" for="backup-retention"
            >{$t("Retention (days)")}</label
          >
          <input
            id="backup-retention"
            class="zf-input"
            type="number"
            min="1"
            bind:value={uploadSettings.backup_retention_days}
            placeholder="30"
          />
          <div class="field-hint">
            {$t(
              "Uploaded files are not automatically deleted from Nextcloud. Manage the shared folder manually to avoid unlimited growth.",
            )}
          </div>
        </div>
      </div>
    </div>
  </div>

  <!-- Report PDF Upload -->
  <div class="zf-card" style="padding:20px;margin-bottom:16px">
    <div class="field-card-title">{$t("Report PDF Upload")}</div>
    <div class="field-group">
      <div class="field-row">
        <div>
          <label class="zf-label" style="display:flex;align-items:center;gap:8px">
            <input type="checkbox" bind:checked={uploadSettings.report_upload_enabled} />
            {$t("Enable report PDF upload")}
          </label>
          <div class="field-hint">
            {$t(
              "On the configured day of each month, an individual timesheet PDF is queued for every employee. Each PDF is uploaded as soon as the employee has fully submitted all their weeks — late submitters are automatically caught up on the next daily check.",
            )}
          </div>
        </div>
      </div>

      <div class="field-row">
        <div>
          <label class="zf-label" for="report-upload-url"
            >{$t("Share link (https://…/s/…)")}</label
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
            {$t("Share password (optional)")}
            {#if uploadSettings.report_upload_password_set}
              <span style="font-size:11px;color:var(--text-tertiary);font-weight:normal"
                >({$t("stored")})</span
              >
            {/if}
          </label>
          <input
            id="report-upload-password"
            class="zf-input"
            type="password"
            bind:value={reportUploadPassword}
            on:input={() => (clearReportPassword = false)}
            placeholder={uploadSettings.report_upload_password_set ? "********" : ""}
            autocomplete="new-password"
            disabled={!uploadSettings.report_upload_enabled}
          />
          {#if uploadSettings.report_upload_password_set}
            <label
              class="zf-label"
              style="display:flex;align-items:center;gap:8px;margin-top:8px"
            >
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
            >{$t("Upload day of month (1–28)")}</label
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
      <div class="field-row" style="justify-content:flex-end">
        <button
          class="zf-btn"
          on:click={runNow}
          disabled={uploading || saving || !uploadSettings.report_upload_enabled}
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

  <!-- Actions -->
  <div class="zf-card" style="padding:20px">
    <div style="display:flex;justify-content:flex-end;gap:8px">
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
