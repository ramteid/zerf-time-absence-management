<script>
  import { api } from "../api.js";
  import { currentUser, settings as appSettings, toast } from "../stores.js";
  import { LANGUAGES, setLanguage, t } from "../i18n.js";

  let settingsForm = {};
  let saving = false;
  let adminFirstName = "";
  let adminLastName = "";
  $: isFirstSetup = !!$currentUser?.must_configure_settings;
  $: needsName =
    isFirstSetup &&
    (!$currentUser?.first_name?.trim() || !$currentUser?.last_name?.trim());

  let countries = [];
  let countryRegions = [];
  let regionsCountry = null;
  let regionLoadId = 0;
  let regionLoading = false;
  let regionsLoadFailed = false;
  const languageOptions = Object.entries(LANGUAGES);
  const timezoneOptions =
    typeof Intl !== "undefined" && typeof Intl.supportedValuesOf === "function"
      ? Intl.supportedValuesOf("timeZone")
      : ["Europe/Berlin", "UTC", "Europe/London", "America/New_York", "America/Los_Angeles", "Asia/Tokyo"];

  function sortCountriesByName(items) {
    return [...items].sort((a, b) => a.name.localeCompare(b.name));
  }

  async function loadRegionsFor(country) {
    if (!country) return [];
    return await api(`/holidays/regions/${country}`);
  }

  async function syncRegionsFor(country) {
    const normalizedCountry = country || "";
    const loadId = ++regionLoadId;
    if (!normalizedCountry) {
      countryRegions = [];
      regionLoading = false;
      regionsLoadFailed = false;
      return;
    }
    regionLoading = true;
    regionsLoadFailed = false;
    try {
      const regions = await loadRegionsFor(normalizedCountry);
      if (loadId !== regionLoadId || normalizedCountry !== (settingsForm.country || "")) {
        return;
      }
      countryRegions = regions;
      const currentRegion = settingsForm.region || "";
      if (currentRegion && !regions.includes(currentRegion)) {
        settingsForm = { ...settingsForm, region: "" };
      }
    } catch {
      if (loadId !== regionLoadId || normalizedCountry !== (settingsForm.country || "")) {
        return;
      }
      countryRegions = [];
      regionsLoadFailed = true;
    } finally {
      if (loadId === regionLoadId && normalizedCountry === (settingsForm.country || "")) {
        regionLoading = false;
      }
    }
  }

  $: selectedCountry = settingsForm.country || "";
  $: if (selectedCountry !== regionsCountry) {
    regionsCountry = selectedCountry;
    void syncRegionsFor(selectedCountry);
  }

  async function load() {
    const [loadedSettings, allCountries] = await Promise.all([
      api("/settings"),
      api("/holidays/countries"),
    ]);
    if (!loadedSettings.timezone) {
      loadedSettings.timezone = "Europe/Berlin";
    }
    settingsForm = loadedSettings;
    appSettings.set(loadedSettings);
    if (settingsForm.ui_language) setLanguage(settingsForm.ui_language);
    countries = sortCountriesByName(allCountries);
  }
  load();

  async function save() {
    if (needsName) {
      if (!adminFirstName.trim() || !adminLastName.trim()) {
        toast($t("Please enter your first name and last name."), "error");
        return;
      }
    }
    if (!settingsForm.country) {
      toast($t("Please select a country."), "error");
      return;
    }
    if (!settingsForm.timezone) {
      toast($t("Please select a timezone."), "error");
      return;
    }
    if (regionLoading) {
      toast($t("Please wait for regions to load."), "error");
      return;
    }
    if (settingsForm.default_weekly_hours == null || settingsForm.default_weekly_hours === "") {
      toast($t("Please enter default weekly hours."), "error");
      return;
    }
    if (
      settingsForm.default_annual_leave_days == null ||
      settingsForm.default_annual_leave_days === ""
    ) {
      toast($t("Please enter default annual leave days."), "error");
      return;
    }
    saving = true;
    try {
      // Normalize the carryover expiry date: send null when the field is empty so the
      // backend treats it as "no date" rather than trying to parse an empty string.
      const body = {
        ...settingsForm,
        carryover_expiry_date: settingsForm.carryover_expiry_date?.trim() || null,
      };
      const saved = await api("/settings", { method: "PUT", body });
      settingsForm = saved;
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
        currentUser.update((userState) => ({
          ...userState,
          first_name: adminFirstName.trim(),
          last_name: adminLastName.trim(),
        }));
      }
      if (isFirstSetup) {
        currentUser.update((userState) => ({ ...userState, must_configure_settings: false }));
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
      class="zf-card"
      style="padding:16px 20px;margin-bottom:16px;border-color:var(--warning)"
    >
      <strong style="color:var(--warning-text)"
        >{$t("Initial setup required.")}</strong
      >
      <p style="font-size:13px;color:var(--text-tertiary);margin-top:4px">
        {$t(
          needsName
            ? "Please enter your name and configure the country, default weekly hours and default annual leave days before using the application."
            : "Please configure the country, default weekly hours and default annual leave days before using the application.",
        )}
      </p>
    </div>
  {/if}
  {#if needsName}
    <div class="zf-card" style="padding:20px;margin-bottom:16px">
      <div style="font-size:14px;font-weight:400;margin-bottom:14px">
        {$t("Your Name")}
      </div>
      <div class="field-group">
        <div class="field-row">
          <div>
            <label class="zf-label" for="admin-first-name"
              >{$t("First name")}</label
            >
            <input
              id="admin-first-name"
              class="zf-input"
              type="text"
              maxlength="200"
              bind:value={adminFirstName}
              required
            />
          </div>
          <div>
            <label class="zf-label" for="admin-last-name"
              >{$t("Last name")}</label
            >
            <input
              id="admin-last-name"
              class="zf-input"
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
  <div class="zf-card" style="padding:20px;margin-bottom:16px">
    <div style="font-size:14px;font-weight:400;margin-bottom:14px">
      {$t("Organization")}
    </div>
    <div class="field-group">
      <div class="field-row">
        <div>
          <label class="zf-label" for="settings-org-name"
            >{$t("Organization name")}</label
          >
          <input
            id="settings-org-name"
            class="zf-input"
            type="text"
            maxlength="200"
            bind:value={settingsForm.organization_name}
            placeholder={$t("e.g. My Company")}
          />
          <div style="font-size:11px;color:var(--text-tertiary);margin-top:4px">
            {$t("Shown on the login screen and in the navigation.")}
          </div>
        </div>
      </div>
    </div>
  </div>
  <div class="zf-card" style="padding:20px;margin-bottom:16px">
    <div style="font-size:14px;font-weight:400;margin-bottom:14px">
      {$t("General")}
    </div>
    <div class="field-group">
      <div class="field-row">
        <div>
          <label class="zf-label" for="settings-language"
            >{$t("Language")}</label
          >
          <select
            id="settings-language"
            class="zf-select"
            bind:value={settingsForm.ui_language}
          >
            {#each languageOptions as [code, language]}
              <option value={code}>{language.label}</option>
            {/each}
          </select>
        </div>
        <div>
          <label class="zf-label" for="settings-time-format"
            >{$t("Time format")}</label
          >
          <select
            id="settings-time-format"
            class="zf-select"
            bind:value={settingsForm.time_format}
          >
            <option value="24h">24h (14:30)</option>
            <option value="12h">12h (2:30 PM)</option>
          </select>
        </div>
        <div>
          <label class="zf-label" for="settings-timezone"
            >{$t("Timezone")}</label
          >
          <select
            id="settings-timezone"
            class="zf-select"
            bind:value={settingsForm.timezone}
          >
            {#each timezoneOptions as tz}
              <option value={tz}>{tz}</option>
            {/each}
          </select>
        </div>
      </div>

      <!-- Default user settings -->
      <div
        style="font-size:14px;font-weight:400;margin-top:20px;margin-bottom:14px"
      >
        {$t("Default weekly hours")} / {$t("Default annual leave days")}
      </div>
      <div class="field-row">
        <div>
          <label class="zf-label" for="settings-default-hours"
            >{$t("Default weekly hours")}</label
          >
          <input
            id="settings-default-hours"
            class="zf-input"
            type="number"
            step="0.5"
            min="0"
            max="168"
            bind:value={settingsForm.default_weekly_hours}
          />
        </div>
        <div>
          <label class="zf-label" for="settings-default-leave"
            >{$t("Default annual leave days")}</label
          >
          <input
            id="settings-default-leave"
            class="zf-input"
            type="number"
            min="0"
            max="366"
            bind:value={settingsForm.default_annual_leave_days}
          />
        </div>
      </div>

      <!-- Carryover expiry date -->
      <div
        style="font-size:14px;font-weight:400;margin-top:20px;margin-bottom:14px"
      >
        {$t("Vacation carryover")}
      </div>
      <div class="field-row">
        <div>
          <label class="zf-label" for="settings-carryover-expiry"
            >{$t("Carryover expiry date (MM-DD)")}</label
          >
          <input
            id="settings-carryover-expiry"
            class="zf-input"
            bind:value={settingsForm.carryover_expiry_date}
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
        style="font-size:14px;font-weight:400;margin-top:20px;margin-bottom:14px"
      >
        {$t("Time submission deadline")}
      </div>
      <div class="field-row">
        <div>
          <label class="zf-label" for="settings-submission-deadline"
            >{$t("Submission deadline day of month")}</label
          >
          <input
            id="settings-submission-deadline"
            class="zf-input"
            type="number"
            min="1"
            max="28"
            bind:value={settingsForm.submission_deadline_day}
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
        style="font-size:14px;font-weight:400;margin-top:20px;margin-bottom:14px"
      >
        {$t("Holidays")}
      </div>
      <div class="field-row">
        <div>
          <label class="zf-label" for="settings-country">{$t("Country")}</label>
          <select
            id="settings-country"
            class="zf-select"
            bind:value={settingsForm.country}
            on:change={() => {
              settingsForm = { ...settingsForm, region: "" };
            }}
          >
            <option value="">{$t("- Please select -")}</option>
            {#each countries as countryOption}
              <option value={countryOption.countryCode}>{countryOption.name}</option>
            {/each}
          </select>
        </div>
        <div>
          <label class="zf-label" for="settings-region">{$t("Region")}</label>
          <select
              id="settings-region"
              class="zf-select"
              bind:value={settingsForm.region}
              disabled={!settingsForm.country || regionLoading || regionsLoadFailed || countryRegions.length === 0}
            >
              {#if !settingsForm.country}
                <option value="">{$t("- Please select -")}</option>
              {:else if regionLoading}
                <option value="">{$t("Loading...")}</option>
              {:else if regionsLoadFailed}
                <option value="">{$t("Could not load regions.")}</option>
              {:else if countryRegions.length === 0}
                <option value="">{$t("No regions available.")}</option>
              {:else}
                <option value="">{$t("- Please select -")}</option>
              {/if}
              {#each countryRegions as regionOption}
                <option value={regionOption}>{regionOption}</option>
              {/each}
            </select>
        </div>
      </div>

      <div style="display:flex;justify-content:flex-end;padding-top:16px">
        <button class="zf-btn zf-btn-primary" on:click={save} disabled={saving || regionLoading}>
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
