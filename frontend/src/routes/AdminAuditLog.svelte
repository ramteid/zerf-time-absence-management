<script>
  import { api } from "../api.js";
  import { t } from "../i18n.js";
  import { fmtDateTime } from "../format.js";

  let log = [];

  async function load() {
    log = await api("/audit-log");
  }
  load();
</script>

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t("Audit Log")}</h1>
  </div>
</div>

<div class="content-area">
  <div class="kz-card" style="overflow:hidden">
    <table class="kz-table">
      <thead>
        <tr>
          <th>{$t("Time")}</th>
          <th>{$t("User")}</th>
          <th>{$t("Table")}</th>
          <th>{$t("Record")}</th>
          <th>{$t("Action")}</th>
        </tr>
      </thead>
      <tbody>
        {#each log as e}
          <tr>
            <td class="tab-num" style="white-space:nowrap"
              >{fmtDateTime(e.created_at)}</td
            >
            <td>{e.user_email || "–"}</td>
            <td>{e.table_name}</td>
            <td class="tab-num">{e.record_id}</td>
            <td>{e.action}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
</div>
