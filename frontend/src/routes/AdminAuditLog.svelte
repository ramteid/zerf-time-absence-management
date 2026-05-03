<script>
  import { api } from "../api.js";
  import { t, auditTableLabel, auditActionLabel } from "../i18n.js";
  import { fmtDateTime } from "../format.js";

  let log = [];
  let usersById = new Map();

  async function load() {
    const [entries, users] = await Promise.all([
      api("/audit-log"),
      api("/users"),
    ]);
    log = entries;
    usersById = new Map(
      users.map((user) => [
        user.id,
        `${user.first_name || ""} ${user.last_name || ""}`.trim(),
      ]),
    );
  }
  load();

  function userLabel(userId) {
    return usersById.get(userId) || (userId == null ? "System" : `#${userId}`);
  }

  function dataSummary(entry) {
    // For delete: show before_data; for create/update: show after_data
    const raw =
      entry.action === "deleted" ? entry.before_data : entry.after_data;
    if (!raw) return "–";
    try {
      const obj = typeof raw === "string" ? JSON.parse(raw) : raw;
      // Pick a few meaningful fields for summary
      const keys = [
        "name",
        "email",
        "kind",
        "status",
        "entry_date",
        "start_date",
        "end_date",
        "start_time",
        "end_time",
        "role",
        "key",
        "value",
      ];
      const parts = [];
      for (const k of keys) {
        if (obj[k] != null) parts.push(`${k}: ${obj[k]}`);
      }
      return parts.length > 0 ? parts.join(", ") : $t("Data");
    } catch {
      return $t("Data");
    }
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
            <th>{$t("Action")}</th>
            <th>{$t("Category")}</th>
            <th>{$t("Data")}</th>
          </tr>
        </thead>
        <tbody>
          {#each log as e}
            <tr>
              <td class="tab-num" style="white-space:nowrap"
                >{fmtDateTime(e.occurred_at)}</td
              >
              <td>{userLabel(e.user_id)}</td>
              <td>{auditActionLabel(e.action)}</td>
              <td>{auditTableLabel(e.table_name)}</td>
              <td
                style="max-width:300px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-size:12px;color:var(--text-tertiary)"
                title={dataSummary(e)}>{dataSummary(e)}</td
              >
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  </div>
</div>
