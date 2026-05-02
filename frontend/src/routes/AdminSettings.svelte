<script>
  import { api } from "../api.js";
  import { toast } from "../stores.js";
  import { t } from "../i18n.js";

  let s = {};

  async function load() {
    s = await api("/settings");
  }
  load();

  async function save() {
    await api("/settings", { method: "PUT", body: s });
    toast($t("Settings saved."), "ok");
  }
</script>

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t("General Settings")}</h1>
  </div>
</div>

<div class="content-area" style="max-width:600px">
  <div class="kz-card" style="padding:20px">
    <div style="font-size:14px;font-weight:600;margin-bottom:14px">
      {$t("General")}
    </div>
    <div class="field-group">
      <div class="field-row">
        <div>
          <label class="kz-label" for="settings-kita-name"
            >{$t("Kita name")}</label
          >
          <input
            id="settings-kita-name"
            class="kz-input"
            bind:value={s.kita_name}
          />
        </div>
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
      <div style="display:flex;justify-content:flex-end;padding-top:4px">
        <button class="kz-btn kz-btn-primary" on:click={save}
          >{$t("Save Changes")}</button
        >
      </div>
    </div>
  </div>
</div>
