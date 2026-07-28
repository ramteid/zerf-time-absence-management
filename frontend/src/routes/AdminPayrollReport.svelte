<script>
  import { api } from "../api.js";
  import { toast } from "../stores.js";
  import { t } from "../i18n.js";

  let settings = {};
  // Absence categories offered as checkboxes; loaded once with the settings.
  let categories = [];
  // Selected category slugs, kept as a plain array so the payload order is stable.
  let selectedSlugs = [];
  let saving = false;
  let sending = false;

  async function load() {
    const [loadedSettings, loadedCategories] = await Promise.all([
      api("/settings"),
      api("/absence-categories/all"),
    ]);
    settings = loadedSettings;
    categories = loadedCategories;
    selectedSlugs = [
      ...(loadedSettings.payroll_report_absence_categories || []),
    ];
  }
  load();

  function toggleCategory(slug, checked) {
    selectedSlugs = checked
      ? [...selectedSlugs, slug]
      : selectedSlugs.filter((entry) => entry !== slug);
  }

  // The report must have a recipient and at least one section before it can be
  // switched on — the backend rejects anything else, so mirror it here for a
  // direct error message instead of a round-trip.
  $: hasContent =
    selectedSlugs.length > 0 ||
    !!settings.payroll_report_include_assistant_hours ||
    !!settings.payroll_report_include_employee_hours;

  async function save() {
    if (settings.payroll_report_enabled) {
      if (!(settings.payroll_report_recipient || "").trim()) {
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
        payroll_report_recipient: (
          settings.payroll_report_recipient || ""
        ).trim(),
        payroll_report_day_of_month:
          parseInt(settings.payroll_report_day_of_month) || 5,
        payroll_report_absence_categories: selectedSlugs,
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
      selectedSlugs = [...(saved.payroll_report_absence_categories || [])];
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
          <label class="zf-label" for="payroll-recipient"
            >{$t("Recipient email address")}</label
          >
          <input
            id="payroll-recipient"
            class="zf-input"
            type="email"
            bind:value={settings.payroll_report_recipient}
            placeholder="lohn@steuerbuero.example"
            disabled={!settings.payroll_report_enabled}
          />
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
          <div class="category-options">
            {#each categories as category (category.slug)}
              <label class="zf-label zf-row">
                <input
                  type="checkbox"
                  checked={selectedSlugs.includes(category.slug)}
                  on:change={(event) =>
                    toggleCategory(category.slug, event.currentTarget.checked)}
                />
                {$t(category.name)}
                {#if !category.active}
                  <span class="zf-label-hint">({$t("inactive")})</span>
                {/if}
              </label>
            {/each}
          </div>
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
    <div class="field-hint">
      {$t(
        "Send now prepares the previous month immediately and sends it if the month is already final. It does not replace the scheduled monthly run.",
      )}
    </div>
  </div>
</div>

<style>
  /* Category checkboxes line up in a fixed grid (instead of free-flowing
     flex-wrap) so entries stay aligned into clean rows/columns regardless of
     label length — flex-wrap let longer German translations push later
     entries into ragged, inconsistent positions. Collapses to a single
     column once the viewport is too narrow for a second one. */
  .category-options {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: 8px 24px;
    margin-top: 8px;
  }

  /* Category names are free text (admin-defined) and unbounded in length —
     let long ones wrap within their grid column instead of overflowing it. */
  .category-options label {
    flex-wrap: wrap;
  }

  .form-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
