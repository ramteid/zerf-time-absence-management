<script>
  import { api } from "../api.js";
  import { currentUser, settings as appSettings, toast } from "../stores.js";
  import { LANGUAGES, setLanguage, t } from "../i18n.js";

  let s = {};
  let saving = false;
  let adminFirstName = "";
  let adminLastName = "";
  $: isFirstSetup = !!$currentUser?.must_configure_settings;
  $: needsName =
    isFirstSetup &&
    (!$currentUser?.first_name?.trim() || !$currentUser?.last_name?.trim());

  let countries = [];
  let countryRegions = [];
  const languageOptions = Object.entries(LANGUAGES);

  function sortCountriesByName(items) {
    return [...items].sort((a, b) => a.name.localeCompare(b.name));
  }

  async function loadRegionsFor(country) {
    if (!country) return [];
    try {
      return await api(`/holidays/regions/${country}`);
    } catch {
      return [];
    }
  }

  async function load() {
    const [loadedSettings, allCountries] = await Promise.all([
      api("/settings"),
      api("/holidays/countries"),
    ]);
    s = loadedSettings;
    appSettings.set(loadedSettings);
    if (s.ui_language) setLanguage(s.ui_language);
    countries = sortCountriesByName(allCountries);
    if (s.country) {
      countryRegions = await loadRegionsFor(s.country);
    }
  }
  load();

  async function save() {
    if (needsName) {
      if (!adminFirstName.trim() || !adminLastName.trim()) {
        toast($t("Please enter your first name and last name."), "error");
        return;
      }
    }
    if (!s.country) {
      toast($t("Please select a country."), "error");
      return;
    }
    if (s.default_weekly_hours == null || s.default_weekly_hours === "") {
      toast($t("Please enter default weekly hours."), "error");
      return;
    }
    if (
      s.default_annual_leave_days == null ||
      s.default_annual_leave_days === ""
    ) {
      toast($t("Please enter default annual leave days."), "error");
      return;
    }
    saving = true;
    try {
      const saved = await api("/settings", { method: "PUT", body: s });
      s = saved;
      appSettings.set(saved);
      if (saved.ui_language) setLanguage(saved.ui_language);
      if (needsName) {
        await api(`/users/${$currentUser.id}`, {
          method: "PUT",
          body: {
            first_name: adminFirstName.trim(),
            last_name: adminLastName.trim(),
          },
        });
        currentUser.update((u) => ({
          ...u,
          first_name: adminFirstName.trim(),
          last_name: adminLastName.trim(),
        }));
      }
      if (isFirstSetup) {
        currentUser.update((u) => ({ ...u, must_configure_settings: false }));
      }
      toast($t("Settings saved."), "ok");
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
    } finally {
      saving = false;
    }
  }
</script>

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t("General Settings")}</h1>
  </div>
</div>

<div class="content-area">
  {#if isFirstSetup}
    <div
      class="kz-card"
      style="padding:16px 20px;margin-bottom:16px;border-color:var(--warning)"
    >
      <strong style="color:var(--warning-text)"
        >{$t("Initial setup required.")}</strong
      >
      <p style="font-size:13px;color:var(--text-tertiary);margin-top:4px">
        {$t(
          needsName
            ? "Please enter your name and configure the country, region, default weekly hours and default annual leave days before using the application."
            : "Please configure the country, region, default weekly hours and default annual leave days before using the application.",
        )}
      </p>
    </div>
  {/if}
  {#if needsName}
    <div class="kz-card" style="padding:20px;margin-bottom:16px">
      <div style="font-size:14px;font-weight:600;margin-bottom:14px">
        {$t("Your Name")}
      </div>
      <div class="field-group">
        <div class="field-row">
          <div>
            <label class="kz-label" for="admin-first-name"
              >{$t("First name")}</label
            >
            <input
              id="admin-first-name"
              class="kz-input"
              type="text"
              maxlength="200"
              bind:value={adminFirstName}
              required
            />
          </div>
          <div>
            <label class="kz-label" for="admin-last-name"
              >{$t("Last name")}</label
            >
            <input
              id="admin-last-name"
              class="kz-input"
              type="text"
              maxlength="200"
              bind:value={adminLastName}
              required
            />
          </div>
        </div>
      </div>
    </div>
  {/if}
  <div class="kz-card" style="padding:20px;margin-bottom:16px">
    <div style="font-size:14px;font-weight:600;margin-bottom:14px">
      {$t("General")}
    </div>
    <div class="field-group">
      <div class="field-row">
        <div>
          <label class="kz-label" for="settings-language"
            >{$t("Language")}</label
          >
          <select
            id="settings-language"
            class="kz-select"
            bind:value={s.ui_language}
          >
            {#each languageOptions as [code, language]}
              <option value={code}>{language.label}</option>
            {/each}
          </select>
        </div>
        <div>
          <label class="kz-label" for="settings-time-format"
            >{$t("Time format")}</label
          >
          <select
            id="settings-time-format"
            class="kz-select"
            bind:value={s.time_format}
          >
            <option value="24h">24h (14:30)</option>
            <option value="12h">12h (2:30 PM)</option>
          </select>
        </div>
      </div>

      <!-- Default user settings -->
      <div
        style="font-size:14px;font-weight:600;margin-top:20px;margin-bottom:14px"
      >
        {$t("Default weekly hours")} / {$t("Default annual leave days")}
      </div>
      <div class="field-row">
        <div>
          <label class="kz-label" for="settings-default-hours"
            >{$t("Default weekly hours")}</label
          >
          <input
            id="settings-default-hours"
            class="kz-input"
            type="number"
            step="0.5"
            min="0"
            max="168"
            bind:value={s.default_weekly_hours}
          />
        </div>
        <div>
          <label class="kz-label" for="settings-default-leave"
            >{$t("Default annual leave days")}</label
          >
          <input
            id="settings-default-leave"
            class="kz-input"
            type="number"
            min="0"
            max="366"
            bind:value={s.default_annual_leave_days}
          />
        </div>
      </div>

      <!-- Carryover expiry date -->
      <div
        style="font-size:14px;font-weight:600;margin-top:20px;margin-bottom:14px"
      >
        {$t("Vacation carryover")}
      </div>
      <div class="field-row">
        <div>
          <label class="kz-label" for="settings-carryover-expiry"
            >{$t("Carryover expiry date (MM-DD)")}</label
          >
          <input
            id="settings-carryover-expiry"
            class="kz-input"
            bind:value={s.carryover_expiry_date}
            placeholder="03-31"
            maxlength="5"
          />
          <div style="font-size:11px;color:var(--text-tertiary);margin-top:4px">
            {$t("Unused vacation from the previous year expires on this date.")}
          </div>
        </div>
      </div>

      <!-- Submission deadline -->
      <div
        style="font-size:14px;font-weight:600;margin-top:20px;margin-bottom:14px"
      >
        {$t("Time submission deadline")}
      </div>
      <div class="field-row">
        <div>
          <label class="kz-label" for="settings-submission-deadline"
            >{$t("Submission deadline day of month")}</label
          >
          <input
            id="settings-submission-deadline"
            class="kz-input"
            type="number"
            min="1"
            max="28"
            bind:value={s.submission_deadline_day}
            placeholder={$t("e.g. 5")}
          />
          <div style="font-size:11px;color:var(--text-tertiary);margin-top:4px">
            {$t(
              "Users will be notified on this day of each month if they have unsubmitted time entries for previous months. Leave empty to disable. (1–28)",
            )}
          </div>
        </div>
      </div>

      <div
        style="font-size:14px;font-weight:600;margin-top:20px;margin-bottom:14px"
      >
        {$t("Holidays")}
      </div>
      <div class="field-row">
        <div>
          <label class="kz-label" for="settings-country">{$t("Country")}</label>
          <select
            id="settings-country"
            class="kz-select"
            bind:value={s.country}
            on:change={async () => {
              s.region = "";
              countryRegions = await loadRegionsFor(s.country);
            }}
          >
            <option value="">{$t("- Please select -")}</option>
            {#each countries as c}
              <option value={c.countryCode}>{c.name}</option>
            {/each}
          </select>
        </div>
        <div>
          <label class="kz-label" for="settings-region">{$t("Region")}</label>
          {#if countryRegions.length > 0}
            <select
              id="settings-region"
              class="kz-select"
              bind:value={s.region}
            >
              <option value="">{$t("- All -")}</option>
              {#each countryRegions as r}
                <option value={r}>{r}</option>
              {/each}
            </select>
          {:else}
            <input
              id="settings-region"
              class="kz-input"
              bind:value={s.region}
              placeholder={$t("e.g. US-CA")}
            />
          {/if}
        </div>
      </div>

      <div style="display:flex;justify-content:flex-end;padding-top:16px">
        <button class="kz-btn kz-btn-primary" on:click={save} disabled={saving}>
          {#if saving}
            {$t("Saving...")}
          {:else}
            {$t("Save Changes")}
          {/if}
        </button>
      </div>
    </div>
  </div>
</div>
