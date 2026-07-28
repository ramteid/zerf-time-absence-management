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
  // includes sick-like and unpaid categories; this list only resolves the
  // returned slugs to their display name and color.
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
      toast($t("Settings saved."), "ok");
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
        toast($t("Report sent."), "ok");
      } else {
        toast(
          $t(
            "No report was sent. It was already sent or the month is not complete.",
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
    <div class="field-card-title">{$t("Automatic delivery")}</div>
    <div class="field-group">
      <div class="field-row">
        <div>
          <label class="zf-label zf-row">
            <input
              type="checkbox"
              bind:checked={settings.payroll_report_enabled}
            />
            {$t("Send the payroll report automatically")}
          </label>
          <div class="field-hint">
            {$t(
              "Sends the previous month's report as a PDF on the selected day. If weeks, absences, or working hours are still open, it is sent later automatically. Email must be set up first.",
            )}
          </div>
        </div>
      </div>

      <div class="field-row">
        <div>
          <label class="zf-label" for="payroll-recipients"
            >{$t("Recipients")}</label
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
              "Enter one email address per line. Everyone receives the same report.",
            )}
          </div>
        </div>
        <div>
          <label class="zf-label" for="payroll-day"
            >{$t("Send day (1-28)")}</label
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
    <div class="field-card-title">{$t("Content")}</div>
    <div class="field-group">
      <div class="field-row">
        <div>
          <span class="zf-label">{$t("Absences")}</span>
          <div class="field-hint">
            {$t("Shows each absence and its number of workdays.")}
          </div>
          <div class="field-hint">
            {$t(
              "Sick and unpaid categories are included automatically. You can change this under Categories.",
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
          <span class="zf-label">{$t("Workdays and hours")}</span>
          <div class="field-hint">
            {$t(
              "Shows each person's workdays and approved hours. Hours are also shown as a decimal.",
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
          "Sends the previous month's report right away if it is complete. It does not replace the automatic delivery — the same report is still sent again automatically on the selected day.",
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
