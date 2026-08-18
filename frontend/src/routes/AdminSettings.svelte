<script>
  import { api } from "../api.js";
  import { currentUser, settings as appSettings, toast } from "../stores.js";
  import {
    LANGUAGES,
    setLanguage,
    t,
    fmtDecimal,
    parseDecimal,
  } from "../i18n.js";

  let settingsForm = {};
  // Locale-formatted string variables for float fields so the decimal separator
  // matches the current UI language (comma for German, period for English).
  let defaultWeeklyHoursStr = "";
  let breakThresholdStr = "";
  let breakThresholdStr2 = "";

  // Sync float string vars from a plain settings object (after load or save).
  function syncFloatStrings(s) {
    defaultWeeklyHoursStr =
      s.default_weekly_hours != null
        ? fmtDecimal(Number(s.default_weekly_hours), 2)
        : "";
    breakThresholdStr =
      s.auto_break_threshold_hours != null
        ? fmtDecimal(Number(s.auto_break_threshold_hours), 2)
        : "";
    breakThresholdStr2 =
      s.auto_break_threshold_hours_2 != null
        ? fmtDecimal(Number(s.auto_break_threshold_hours_2), 2)
        : "";
  }
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
      : [
          "Europe/Berlin",
          "UTC",
          "Europe/London",
          "America/New_York",
          "America/Los_Angeles",
          "Asia/Tokyo",
        ];

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
      if (
        loadId !== regionLoadId ||
        normalizedCountry !== (settingsForm.country || "")
      ) {
        return;
      }
      countryRegions = regions;
      const currentRegion = settingsForm.region || "";
      if (currentRegion && !regions.includes(currentRegion)) {
        settingsForm = { ...settingsForm, region: "" };
      }
    } catch {
      if (
        loadId !== regionLoadId ||
        normalizedCountry !== (settingsForm.country || "")
      ) {
        return;
      }
      countryRegions = [];
      regionsLoadFailed = true;
    } finally {
      if (
        loadId === regionLoadId &&
        normalizedCountry === (settingsForm.country || "")
      ) {
        regionLoading = false;
      }
    }
  }

  $: selectedCountry = settingsForm.country || "";
  $: if (selectedCountry !== regionsCountry) {
    // eslint-disable-next-line no-useless-assignment
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
    // Apply the stored language before formatting floats so the decimal separator
    // matches the saved UI language from the first render.
    if (settingsForm.ui_language) setLanguage(settingsForm.ui_language);
    syncFloatStrings(settingsForm);
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
    if (defaultWeeklyHoursStr == null || defaultWeeklyHoursStr === "") {
      toast($t("Please enter default weekly hours."), "error");
      return;
    }
    if (settingsForm.auto_break_enabled) {
      if (breakThresholdStr == null || breakThresholdStr === "") {
        toast($t("Please enter the break threshold."), "error");
        return;
      }
      if (
        settingsForm.auto_break_deduction_minutes == null ||
        settingsForm.auto_break_deduction_minutes === ""
      ) {
        toast($t("Please enter the break deduction minutes."), "error");
        return;
      }
      const hasTier2Threshold =
        breakThresholdStr2 != null && breakThresholdStr2 !== "";
      const hasTier2Deduction =
        settingsForm.auto_break_deduction_minutes_2 != null &&
        settingsForm.auto_break_deduction_minutes_2 !== "";
      if (hasTier2Threshold !== hasTier2Deduction) {
        toast(
          $t(
            "Please enter both second threshold and second deduction, or leave both empty.",
          ),
          "error",
        );
        return;
      }
    }
    saving = true;
    try {
      const body = {
        ...settingsForm,
        // Parse locale-formatted float strings back to numbers before sending.
        default_weekly_hours: parseDecimal(defaultWeeklyHoursStr),
        // Clear all break values when the feature is disabled.
        auto_break_threshold_hours: settingsForm.auto_break_enabled
          ? parseDecimal(breakThresholdStr)
          : null,
        auto_break_deduction_minutes: settingsForm.auto_break_enabled
          ? settingsForm.auto_break_deduction_minutes
          : null,
        auto_break_threshold_hours_2: settingsForm.auto_break_enabled
          ? parseDecimal(breakThresholdStr2) || null
          : null,
        auto_break_deduction_minutes_2: settingsForm.auto_break_enabled
          ? settingsForm.auto_break_deduction_minutes_2 || null
          : null,
      };
      const saved = await api("/settings", { method: "PUT", body });
      settingsForm = saved;
      // Apply the new language before re-formatting floats so the decimal separator
      // reflects the freshly saved language setting.
      if (saved.ui_language) setLanguage(saved.ui_language);
      syncFloatStrings(saved);
      appSettings.set(saved);
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
        currentUser.update((userState) => ({
          ...userState,
          must_configure_settings: false,
        }));
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
    <div class="zf-card zf-card-warning">
      <strong class="text-warning">{$t("Initial setup required.")}</strong>
      <p class="fs-14 text-tertiary mt-4">
        {$t(
          needsName
            ? "Please enter your name and configure the country and default weekly hours before using the application."
            : "Please configure the country and default weekly hours before using the application.",
        )}
      </p>
    </div>
  {/if}
  {#if needsName}
    <div class="zf-card zf-card-section">
      <div class="zf-card-title mb-14">
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
  <div class="zf-card zf-card-section">
    <div class="field-card-title">{$t("Organization")}</div>
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
          <div class="field-hint">
            {$t("Shown on the login screen and in the navigation.")}
          </div>
        </div>
      </div>
    </div>
  </div>
  <div class="zf-card zf-card-section">
    <div class="field-card-title">{$t("General")}</div>
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
            {#each languageOptions as [code, language] (code)}
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
            {#each timezoneOptions as tz (tz)}
              <option value={tz}>{tz}</option>
            {/each}
          </select>
        </div>
      </div>

      <!-- Default user settings -->
      <div class="zf-form-section-title">
        {$t("Default weekly hours")}
      </div>
      <div class="field-row">
        <div>
          <label class="zf-label" for="settings-default-hours"
            >{$t("Default weekly hours")}</label
          >
          <input
            id="settings-default-hours"
            class="zf-input"
            type="text"
            inputmode="decimal"
            bind:value={defaultWeeklyHoursStr}
          />
        </div>
      </div>

      <!-- Submission deadline -->
      <div class="zf-form-section-title">
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
          <div class="field-hint">
            {$t(
              "Users will be notified on this day of each month if they have unsubmitted time entries for previous months. Leave empty to disable. (1–28)",
            )}
          </div>
        </div>
      </div>

      <!-- Medical certificate (AU) threshold -->
      <div class="zf-form-section-title">
        {$t("Medical certificate (AU)")}
      </div>
      <div class="field-row">
        <div>
          <label class="zf-label" for="settings-medical-certificate-threshold"
            >{$t(
              "Consecutive sick days before a certificate is required",
            )}</label
          >
          <input
            id="settings-medical-certificate-threshold"
            class="zf-input"
            type="number"
            min="1"
            max="30"
            bind:value={settingsForm.medical_certificate_threshold_days}
            placeholder={$t("e.g. 4")}
          />
          <div class="field-hint">
            {$t(
              "Applies only to absence categories marked accordingly under Categories. Counts consecutive calendar days, bridging weekends and public holidays.",
            )}
          </div>
        </div>
      </div>

      <!-- Automatic break deduction -->
      <div class="zf-form-section-title">
        {$t("Automatic break deduction")}
      </div>
      <div class="field-row">
        <div class="flex-none">
          <label class="zf-label zf-check-label">
            <input
              type="checkbox"
              bind:checked={settingsForm.auto_break_enabled}
              on:change={() => {
                if (!settingsForm.auto_break_enabled) {
                  settingsForm = {
                    ...settingsForm,
                    auto_break_threshold_hours: null,
                    auto_break_deduction_minutes: null,
                    auto_break_threshold_hours_2: null,
                    auto_break_deduction_minutes_2: null,
                  };
                  breakThresholdStr = "";
                  breakThresholdStr2 = "";
                }
              }}
            />
            {$t("Enable automatic break deduction")}
          </label>
          <div class="field-hint">
            {$t(
              "When enabled, a break is automatically deducted from time entries that form a continuous work block meeting or exceeding the configured threshold.",
            )}
          </div>
        </div>
      </div>
      {#if settingsForm.auto_break_enabled}
        <div class="field-row mt-10">
          <div>
            <label class="zf-label" for="settings-break-threshold"
              >{$t("Break threshold (hours)")}</label
            >
            <input
              id="settings-break-threshold"
              class="zf-input"
              type="text"
              inputmode="decimal"
              bind:value={breakThresholdStr}
              placeholder={$t("e.g. 6")}
            />
            <div class="field-hint">
              {$t("After how many consecutive hours a break is deducted.")}
            </div>
          </div>
          <div>
            <label class="zf-label" for="settings-break-deduction"
              >{$t("Break deduction (minutes)")}</label
            >
            <input
              id="settings-break-deduction"
              class="zf-input"
              type="number"
              step="1"
              min="1"
              max="480"
              bind:value={settingsForm.auto_break_deduction_minutes}
              placeholder={$t("e.g. 30")}
            />
            <div class="field-hint">
              {$t("How many minutes are deducted per qualifying work block.")}
            </div>
          </div>
        </div>
        <div class="field-row mt-10">
          <div>
            <label class="zf-label" for="settings-break-threshold-2"
              >{$t("Second threshold (hours)")}</label
            >
            <input
              id="settings-break-threshold-2"
              class="zf-input"
              type="text"
              inputmode="decimal"
              bind:value={breakThresholdStr2}
              placeholder={$t("e.g. 9 (optional)")}
            />
            <div class="field-hint">
              {$t(
                "Optional. If the work block reaches this duration, the second deduction applies instead of the first.",
              )}
            </div>
          </div>
          <div>
            <label class="zf-label" for="settings-break-deduction-2"
              >{$t("Second deduction (minutes)")}</label
            >
            <input
              id="settings-break-deduction-2"
              class="zf-input"
              type="number"
              step="1"
              min="1"
              max="480"
              bind:value={settingsForm.auto_break_deduction_minutes_2}
              placeholder={$t("e.g. 45 (optional)")}
            />
            <div class="field-hint">
              {$t(
                "Total minutes deducted when the second threshold is reached.",
              )}
            </div>
          </div>
        </div>
      {/if}

      <div class="zf-form-section-title">
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
            {#each countries as countryOption (countryOption.countryCode)}
              <option value={countryOption.countryCode}
                >{countryOption.name}</option
              >
            {/each}
          </select>
        </div>
        <div>
          <label class="zf-label" for="settings-region">{$t("Region")}</label>
          <select
            id="settings-region"
            class="zf-select"
            bind:value={settingsForm.region}
            disabled={!settingsForm.country ||
              regionLoading ||
              regionsLoadFailed ||
              countryRegions.length === 0}
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
            {#each countryRegions as regionOption (regionOption)}
              <option value={regionOption}>{regionOption}</option>
            {/each}
          </select>
        </div>
      </div>

      <div class="form-actions">
        <button
          class="zf-btn zf-btn-primary"
          on:click={save}
          disabled={saving || regionLoading}
        >
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

<style>
  .form-actions {
    display: flex;
    justify-content: flex-end;
    padding-top: 16px;
  }
</style>
