<script>
  import { tick } from "svelte";
  import { api } from "../api.js";
  import { toast } from "../stores.js";
  import { t, roleLabel } from "../i18n.js";
  import { fmtMonthLabel } from "../format.js";
  import { sortUsersByRoleThenName } from "../lib/domain/users.js";

  let settings = {};
  // Everyone who can appear in the report, for the exclusion list below.
  // Admins are never part of the report, so they are not offered here either.
  let selectableUsers = [];
  // IDs ticked in the "except" list — these people are left out of the report.
  let excludedUserIds = [];
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

  // Why the backend sent nothing, mapped to what the admin should read. The
  // keys are the `skipped` values of POST /settings/payroll-report/send-now.
  const SKIP_MESSAGES = {
    covers_nobody: "Nothing to send for {month} — nobody to report on.",
    nobody_final: "Nothing sent for {month} — nobody has finished the month.",
    nothing_approved: "Nothing to send for {month} — no approved times yet.",
    email_unavailable: "Email is not set up, so nothing could be sent.",
  };

  // Grows the textarea to fit its content instead of scrolling internally.
  function resizeRecipientsTextarea() {
    if (!recipientsTextarea) return;
    recipientsTextarea.style.height = "auto";
    recipientsTextarea.style.height = `${recipientsTextarea.scrollHeight}px`;
  }

  async function load() {
    const [loadedSettings, loadedCategories, loadedUsers] = await Promise.all([
      api("/settings"),
      api("/absence-categories/all"),
      api("/users"),
    ]);
    settings = loadedSettings;
    categories = loadedCategories;
    // Only people who can actually show up in the report: deactivated accounts
    // are filtered out here and archived ones never reach the endpoint, so a
    // former colleague cannot linger in the list. Admins are never reported on.
    selectableUsers = sortUsersByRoleThenName(
      (loadedUsers || []).filter(
        (user) => user.active && user.role !== "admin",
      ),
    );
    excludedUserIds = loadedSettings.payroll_report_excluded_user_ids || [];
    recipientsInput = (loadedSettings.payroll_report_recipients || []).join(
      "\n",
    );
    await tick();
    resizeRecipientsTextarea();
  }
  load();

  // Only IDs that still match a selectable person count — a stale ID left
  // behind by a deleted account must not skew the "N of M included" summary.
  $: excludedVisibleCount = selectableUsers.filter((user) =>
    excludedUserIds.includes(user.id),
  ).length;
  $: includedCount = selectableUsers.length - excludedVisibleCount;

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

  // The month "Send now" will actually send, named on the button so the admin
  // knows before clicking. The backend decides which one it is (the previous
  // month while its report is still owed, otherwise the running month), so
  // this only formats what it reports back.
  $: sendNowMonth = settings.payroll_report_send_now_period
    ? fmtMonthLabel(settings.payroll_report_send_now_period)
    : "";

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
      if (!settings.smtp_enabled) {
        toast(
          $t("Email must be set up before the payroll report can be enabled."),
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
        payroll_report_excluded_user_ids: excludedUserIds,
      };
      const saved = await api("/settings/payroll-report", {
        method: "PUT",
        body,
      });
      settings = saved;
      excludedUserIds = saved.payroll_report_excluded_user_ids || [];
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
      const month = result?.period
        ? fmtMonthLabel(result.period)
        : sendNowMonth;
      // Always give visible feedback. A click that sends nothing used to look
      // exactly like a click that failed, which is what made a seemingly
      // successful send impossible to verify. When nothing goes out the
      // backend says why, because the reasons call for different action —
      // chasing submissions is not the same as waiting for approvals. A real
      // failure throws and lands in the catch below.
      if (result?.sent > 0) {
        toast($t("{month} sent.").replace("{month}", month), "ok");
      } else {
        toast(
          $t(
            SKIP_MESSAGES[result?.skipped] ?? SKIP_MESSAGES.nothing_approved,
          ).replace("{month}", month),
          "info",
        );
      }
    } catch (e) {
      toast(e?.message || $t("Error"), "error");
    } finally {
      sending = false;
    }
    // The target month moves on once a month's report is fully delivered, so
    // refresh what the button label is derived from. Deliberately outside the
    // block above: the send result has already been reported, and a failure to
    // re-read the settings must not contradict it with a second, opposite
    // toast. A stale label until the next page load is the lesser problem.
    try {
      settings = await api("/settings");
    } catch {
      // Keep the label as it was.
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
    <div class="field-card-title">{$t("People included")}</div>
    <div class="field-group">
      <div class="field-row">
        <div>
          <span class="zf-label">{$t("All employees and assistants")}</span>
          <div class="field-hint">
            {$t("Administrators never appear in the payroll report.")}
          </div>

          <span class="zf-label except-label">{$t("except")}</span>
          <div class="field-hint">
            {$t(
              "Anyone ticked here is left out of the report and does not hold up its delivery.",
            )}
          </div>
          {#if selectableUsers.length === 0}
            <div class="field-hint">{$t("No people to select.")}</div>
          {:else}
            <div class="check-list">
              {#each selectableUsers as person (person.id)}
                <label class="zf-check-label">
                  <input
                    type="checkbox"
                    value={person.id}
                    bind:group={excludedUserIds}
                  />
                  {person.first_name}
                  {person.last_name}
                  <span class="zf-label-hint">({roleLabel(person.role)})</span>
                </label>
              {/each}
            </div>
            <div class="field-hint">
              {$t("{included} of {total} people included")
                .replace("{included}", includedCount)
                .replace("{total}", selectableUsers.length)}
            </div>
          {/if}
        </div>
      </div>
    </div>
  </div>

  <div class="zf-card zf-card-section">
    <div class="form-actions">
      <div class="field-hint form-actions-hint">
        {$t(
          "Sends the current state of the named month right away, with the times approved so far. It does not replace the automatic delivery — the complete report is still sent on the selected day.",
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
          {:else if sendNowMonth}
            {$t("Send {month} now").replace("{month}", sendNowMonth)}
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

  /* "except" introduces the exclusion list below it, so it needs air above
     but stays tight to its own hint text. */
  .except-label {
    margin-top: 14px;
  }

  /* Scrollable checkbox list of people, same shape as the team/permission
     lists in UserDialog. */
  .check-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
    max-height: 220px;
    overflow-y: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: 8px;
    margin-top: 8px;
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
