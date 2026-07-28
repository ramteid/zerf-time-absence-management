<script>
  import { tick } from "svelte";
  import { api } from "../api.js";
  import { toast } from "../stores.js";
  import { t } from "../i18n.js";

  let settings = {};
  // Absence categories, loaded once so the read-only "included automatically"
  // list below can show each category's translated name and color.
  let categories = [];
  let saving = false;
  let sending = false;
  // Recipients are edited as one address per line and only split into an
  // array on save/load — a textarea reads more naturally than a
  // comma-separated line once there are more than a couple of addresses.
  let recipientsInput = "";
  let recipientsTextarea;

  // Grows the textarea to fit its content instead of scrolling internally.
  function resizeRecipientsTextarea() {
    if (!recipientsTextarea) return;
    recipientsTextarea.style.height = "auto";
    recipientsTextarea.style.height = `${recipientsTextarea.scrollHeight}px`;
  }

  async function load() {
    const [loadedSettings, loadedCategories] = await Promise.all([
      api("/settings"),
      api("/absence-categories/all"),
    ]);
    settings = loadedSettings;
    categories = loadedCategories;
    recipientsInput = (loadedSettings.payroll_report_recipients || []).join(
      "\n",
    );
    await tick();
    resizeRecipientsTextarea();
  }
  load();

  function parseRecipients(value) {
    return [
      ...new Set(
        value
          .split("\n")
          .map((address) => address.trim())
          .filter((address) => address.length > 0),
      ),
    ];
  }

  // Categories the report currently includes automatically. The backend
  // decides membership from each category's behavior (sick-like, or costing
  // neither vacation nor flextime) — there is nothing to pick here, this just
  // resolves the returned slugs to their display name and color.
  $: includedCategories = categories.filter((category) =>
    (settings.payroll_report_absence_categories || []).includes(category.slug),
  );

  // The report must have a recipient and at least one section before it can be
  // switched on — the backend rejects anything else, so mirror it here for a
  // direct error message instead of a round-trip.
  $: hasContent =
    (settings.payroll_report_absence_categories || []).length > 0 ||
    !!settings.payroll_report_include_assistant_hours ||
    !!settings.payroll_report_include_employee_hours;

  async function save() {
    const recipients = parseRecipients(recipientsInput);
    if (settings.payroll_report_enabled) {
      if (recipients.length === 0) {
        toast(
          $t("A recipient address is required to enable the payroll report."),
          "error",
        );
        return;
      }
      if (!hasContent) {
        toast(
          $t("Select at least one section for the payroll report."),
          "error",
        );
        return;
      }
    }
    saving = true;
    try {
      const body = {
        payroll_report_enabled: !!settings.payroll_report_enabled,
        payroll_report_recipients: recipients,
        payroll_report_day_of_month:
          parseInt(settings.payroll_report_day_of_month) || 5,
        payroll_report_include_assistant_hours:
          !!settings.payroll_report_include_assistant_hours,
        payroll_report_include_employee_hours:
          !!settings.payroll_report_include_employee_hours,
      };
      const saved = await api("/settings/payroll-report", {
        method: "PUT",
        body,
      });
      settings = saved;
      recipientsInput = (saved.payroll_report_recipients || []).join("\n");
      await tick();
      resizeRecipientsTextarea();
      toast($t("Payroll report settings saved."), "ok");
    } catch (e) {
      toast(e?.message || $t("Error"), "error");
    } finally {
      saving = false;
    }
  }

  async function sendNow() {
    sending = true;
    try {
      const result = await api("/settings/payroll-report/send-now", {
        method: "POST",
      });
      // Nothing sent means either every month went out already or a month is
      // still open — the admins who opted in were notified with the details.
      if (result?.sent > 0) {
        toast($t("Payroll report sent."), "ok");
      } else {
        toast(
          $t(
            "Nothing was sent: every month was already sent or is not final yet.",
          ),
          "info",
        );
      }
    } catch (e) {
      toast(e?.message || $t("Error"), "error");
    } finally {
      sending = false;
    }
  }
</script>

<div class="top-bar page-medium">
  <div class="top-bar-title">
    <h1>{$t("Payroll Report")}</h1>
  </div>
</div>

<div class="content-area page-medium">
  <div class="zf-card zf-card-section">
    <div class="field-card-title">{$t("Monthly payroll report")}</div>
    <div class="field-group">
      <div class="field-row">
        <div>
          <label class="zf-label zf-row">
            <input
              type="checkbox"
              bind:checked={settings.payroll_report_enabled}
            />
            {$t("Send the payroll report by email")}
          </label>
          <div class="field-hint">
            {$t(
              "On the configured day of each month, the previous month's report is prepared and emailed as a PDF. It is only sent once every employee's month is final: weeks submitted, absence requests decided, and — for everyone whose hours are in the report — all time entries approved. Otherwise the report waits and is retried daily. Requires a configured email server.",
            )}
          </div>
        </div>
      </div>

      <div class="field-row">
        <div>
          <label class="zf-label" for="payroll-recipients"
            >{$t("Recipient email addresses")}</label
          >
          <textarea
            id="payroll-recipients"
            class="zf-textarea"
            rows="1"
            bind:this={recipientsTextarea}
            bind:value={recipientsInput}
            on:input={resizeRecipientsTextarea}
            placeholder="lohn@steuerbuero.example
buchhaltung@example.com"
            disabled={!settings.payroll_report_enabled}></textarea>
          <div class="field-hint">
            {$t(
              "One address per line. Every recipient receives the same email.",
            )}
          </div>
        </div>
        <div>
          <label class="zf-label" for="payroll-day"
            >{$t("Send day of month (1–28)")}</label
          >
          <input
            id="payroll-day"
            class="zf-input"
            type="number"
            min="1"
            max="28"
            bind:value={settings.payroll_report_day_of_month}
            placeholder="5"
            disabled={!settings.payroll_report_enabled}
          />
        </div>
      </div>
    </div>
  </div>

  <div class="zf-card zf-card-section">
    <div class="field-card-title">{$t("Report content")}</div>
    <div class="field-group">
      <div class="field-row">
        <div>
          <span class="zf-label">{$t("Absence days per employee")}</span>
          <div class="field-hint">
            {$t(
              "One row per absence period with the number of working days. Sick days are needed for health-insurance reimbursement, unpaid days reduce the salary payout.",
            )}
          </div>
          <div class="field-hint">
            {$t(
              "Included automatically — sick-like categories, and any category that counts neither as vacation nor as flextime. Nothing to select here; manage the behavior on the Categories page.",
            )}
          </div>
          {#if includedCategories.length > 0}
            <div class="category-list">
              {#each includedCategories as category (category.slug)}
                <span class="category-chip">
                  <span class="cat-dot" style:background={category.color}
                  ></span>
                  {$t(category.name)}
                  {#if !category.active}
                    <span class="zf-label-hint">({$t("inactive")})</span>
                  {/if}
                </span>
              {/each}
            </div>
          {/if}
        </div>
      </div>

      <div class="field-row">
        <div>
          <span class="zf-label">{$t("Working days and hours")}</span>
          <div class="field-hint">
            {$t(
              "Worked days and approved hours per person, shown in hours:minutes and as a decimal value for payroll.",
            )}
          </div>
          <label class="zf-label zf-row">
            <input
              type="checkbox"
              bind:checked={settings.payroll_report_include_assistant_hours}
            />
            {$t("Assistants")}
          </label>
          <label class="zf-label zf-row">
            <input
              type="checkbox"
              bind:checked={settings.payroll_report_include_employee_hours}
            />
            {$t("All other employees")}
          </label>
        </div>
      </div>
    </div>
  </div>

  <div class="zf-card zf-card-section">
    <div class="form-actions">
      <div class="field-hint form-actions-hint">
        {$t(
          "Send now prepares the previous month immediately and sends it if the month is already final. It does not replace the scheduled monthly run.",
        )}
      </div>
      <div class="form-actions-buttons">
        <button
          class="zf-btn zf-btn-accent-soft"
          on:click={sendNow}
          disabled={sending || saving || !settings.payroll_report_enabled}
        >
          {#if sending}
            {$t("Sending...")}
          {:else}
            {$t("Send now")}
          {/if}
        </button>
        <button
          class="zf-btn zf-btn-primary"
          on:click={save}
          disabled={saving || sending}
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
</div>

<style>
  /* Grown via JS to fit its content on every keystroke, so a scrollbar
     should never appear even for the one frame before the height updates. */
  #payroll-recipients {
    overflow-y: hidden;
  }

  /* Read-only list of categories the report includes automatically — a
     wrapping row of small color-dot + name chips, not an input. */
  .category-list {
    display: flex;
    flex-wrap: wrap;
    gap: 8px 20px;
    margin-top: 8px;
  }

  .category-chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 0.8125rem;
    color: var(--text-secondary);
  }

  /* Matches the Nextcloud Backups page's form-actions spacing (gap,
     padding-top); the explanatory text sits beside the buttons. */
  .form-actions {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 16px;
    padding-top: 16px;
  }

  .form-actions-hint {
    margin-top: 0;
  }

  .form-actions-buttons {
    display: flex;
    flex-shrink: 0;
    gap: 8px;
  }

  /* Too narrow for hint text and both buttons on one line — stack them. */
  @media (max-width: 768px) {
    .form-actions {
      flex-direction: column;
      align-items: stretch;
    }

    .form-actions-buttons {
      justify-content: flex-end;
    }
  }
</style>
