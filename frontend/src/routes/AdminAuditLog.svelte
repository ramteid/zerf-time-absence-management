<script>
  import { api } from "../api.js";
  import { t } from "../i18n.js";
  import { fmtDateTime } from "../format.js";

  let log = [];
  let usersById = new Map();

  async function load() {
    const [entries, users] = await Promise.all([
      api("/audit-log"),
      api("/users"),
    ]);
    log = entries;
    usersById = new Map(users.map((user) => [user.id, user.email]));
  }
  load();

  function userLabel(userId) {
    return usersById.get(userId) || `#${userId}`;
  }
</script>

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t("Audit Log")}</h1>
  </div>
</div>

<div class="content-area">
  <div class="kz-card" style="overflow-x:auto">
    <div class="kz-table-wrap">
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
                >{fmtDateTime(e.occurred_at)}</td
              >
              <td>{userLabel(e.user_id)}</td>
              <td>{e.table_name}</td>
              <td class="tab-num">{e.record_id}</td>
              <td>{e.action}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    <!-- end kz-table-wrap -->
  </div>
</div>
