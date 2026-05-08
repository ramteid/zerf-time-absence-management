<script>
  import { api } from "../api.js";
  import { t, auditTableLabel, auditActionLabel } from "../i18n.js";
  import { fmtDateTime } from "../format.js";

  let log = [];
  let usersById = new Map();
  let rows = [];

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

  function userLabel(userId, userMap) {
    return userMap.get(userId) || (userId == null ? "System" : `#${userId}`);
  }

  function dataSummary(entry, translate) {
    // For delete: show before_data; for create/update: show after_data
    const raw =
      entry.action === "deleted" ? entry.before_data : entry.after_data;
    if (!raw) return "–";
    try {
      const parsedData = typeof raw === "string" ? JSON.parse(raw) : raw;
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
      for (const fieldKey of keys) {
        if (parsedData[fieldKey] != null) parts.push(`${fieldKey}: ${parsedData[fieldKey]}`);
      }
      return parts.length > 0 ? parts.join(", ") : translate("Data");
    } catch {
      return translate("Data");
    }
  }

  $: rows = log.map((entry) => ({
    ...entry,
    user_label: userLabel(entry.user_id, usersById),
    data_summary: dataSummary(entry, $t),
  }));
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
          {#each rows as auditEntry}
            <tr>
              <td class="tab-num" style="white-space:nowrap"
                >{fmtDateTime(auditEntry.occurred_at)}</td
              >
              <td>{auditEntry.user_label}</td>
              <td>{auditActionLabel(auditEntry.action)}</td>
              <td>{auditTableLabel(auditEntry.table_name)}</td>
              <td
                style="max-width:300px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-size:12px;color:var(--text-tertiary)"
                title={auditEntry.data_summary}>{auditEntry.data_summary}</td
              >
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  </div>
</div>
