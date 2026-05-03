<script>
  import { api } from "../api.js";
  import { toast } from "../stores.js";
  import { t } from "../i18n.js";

  let s = {};
  let saving = false;

  const countries = [
    { code: "AT", name: "Österreich / Austria" },
    { code: "CH", name: "Schweiz / Switzerland" },
    { code: "DE", name: "Deutschland / Germany" },
    { code: "FR", name: "France" },
    { code: "IT", name: "Italia / Italy" },
    { code: "NL", name: "Nederland / Netherlands" },
    { code: "PL", name: "Polska / Poland" },
    { code: "CZ", name: "Česko / Czech Republic" },
    { code: "US", name: "United States" },
    { code: "GB", name: "United Kingdom" },
  ];

  const regions = {
    DE: [
      { code: "", label: "– Alle / All –" },
      { code: "DE-BW", label: "Baden-Württemberg" },
      { code: "DE-BY", label: "Bayern" },
      { code: "DE-BE", label: "Berlin" },
      { code: "DE-BB", label: "Brandenburg" },
      { code: "DE-HB", label: "Bremen" },
      { code: "DE-HH", label: "Hamburg" },
      { code: "DE-HE", label: "Hessen" },
      { code: "DE-MV", label: "Mecklenburg-Vorpommern" },
      { code: "DE-NI", label: "Niedersachsen" },
      { code: "DE-NW", label: "Nordrhein-Westfalen" },
      { code: "DE-RP", label: "Rheinland-Pfalz" },
      { code: "DE-SL", label: "Saarland" },
      { code: "DE-SN", label: "Sachsen" },
      { code: "DE-ST", label: "Sachsen-Anhalt" },
      { code: "DE-SH", label: "Schleswig-Holstein" },
      { code: "DE-TH", label: "Thüringen" },
    ],
    AT: [
      { code: "", label: "– Alle / All –" },
      { code: "AT-1", label: "Burgenland" },
      { code: "AT-2", label: "Kärnten" },
      { code: "AT-3", label: "Niederösterreich" },
      { code: "AT-4", label: "Oberösterreich" },
      { code: "AT-5", label: "Salzburg" },
      { code: "AT-6", label: "Steiermark" },
      { code: "AT-7", label: "Tirol" },
      { code: "AT-8", label: "Vorarlberg" },
      { code: "AT-9", label: "Wien" },
    ],
    CH: [
      { code: "", label: "– Alle / All –" },
      { code: "CH-AG", label: "Aargau" },
      { code: "CH-AR", label: "Appenzell Ausserrhoden" },
      { code: "CH-AI", label: "Appenzell Innerrhoden" },
      { code: "CH-BL", label: "Basel-Landschaft" },
      { code: "CH-BS", label: "Basel-Stadt" },
      { code: "CH-BE", label: "Bern" },
      { code: "CH-FR", label: "Freiburg" },
      { code: "CH-GE", label: "Genf" },
      { code: "CH-GL", label: "Glarus" },
      { code: "CH-GR", label: "Graubünden" },
      { code: "CH-JU", label: "Jura" },
      { code: "CH-LU", label: "Luzern" },
      { code: "CH-NE", label: "Neuenburg" },
      { code: "CH-NW", label: "Nidwalden" },
      { code: "CH-OW", label: "Obwalden" },
      { code: "CH-SG", label: "St. Gallen" },
      { code: "CH-SH", label: "Schaffhausen" },
      { code: "CH-SZ", label: "Schwyz" },
      { code: "CH-SO", label: "Solothurn" },
      { code: "CH-TG", label: "Thurgau" },
      { code: "CH-TI", label: "Tessin" },
      { code: "CH-UR", label: "Uri" },
      { code: "CH-VD", label: "Waadt" },
      { code: "CH-VS", label: "Wallis" },
      { code: "CH-ZG", label: "Zug" },
      { code: "CH-ZH", label: "Zürich" },
    ],
  };

  $: availableRegions = regions[s.country] || [];

  async function load() {
    s = await api("/settings");
  }
  load();

  async function save() {
    saving = true;
    try {
      await api("/settings", { method: "PUT", body: s });
      toast($t("Settings saved."), "ok");
    } catch (e) {
      toast(e.message || $t("Error"), "error");
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

<div class="content-area" style="max-width:600px">
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
            <option value="en">English</option>
            <option value="de">Deutsch</option>
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
            on:change={() => {
              s.region = "";
            }}
          >
            {#each countries as c}
              <option value={c.code}>{c.name}</option>
            {/each}
          </select>
        </div>
        <div>
          <label class="kz-label" for="settings-region">{$t("Region")}</label>
          {#if availableRegions.length > 0}
            <select
              id="settings-region"
              class="kz-select"
              bind:value={s.region}
            >
              {#each availableRegions as r}
                <option value={r.code}>{r.label}</option>
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
