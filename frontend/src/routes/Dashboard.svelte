<script>
  import { api } from "../api.js";
  import { currentUser, toast } from "../stores.js";
  import { t, statusLabel, absenceKindLabel } from "../i18n.js";
  import { fmtDate, fmtDateShort, minToHM, isoDate, addDays } from "../format.js";
  import Icon from "../Icons.svelte";
  import { confirmDialog } from "../confirm.js";
  import FlextimeChart from "../FlextimeChart.svelte";

  let pendingEntries = [];
  let pendingAbsences = [];
  let changeRequests = [];
  let pendingReopens = [];
  let users = [];

  // ── Flextime chart ────────────────────────────────────────────────────────
  const today = new Date();

  function daysAgo(n) {
    return isoDate(addDays(today, -n));
  }

  let chartFrom = daysAgo(29);
  let chartTo = isoDate(today);
  let chartData = [];
  let chartLoading = false;

  async function loadChart() {
    chartLoading = true;
    try {
      chartData = await api(
        `/reports/flextime?from=${chartFrom}&to=${chartTo}`,
      );
    } catch {
      chartData = [];
    } finally {
      chartLoading = false;
    }
  }

  function setRange(days) {
    chartFrom = daysAgo(days - 1);
    chartTo = isoDate(today);
    loadChart();
  }

  // ─────────────────────────────────────────────────────────────────────────

  async function load() {
    const [e, a, c, r, u] = await Promise.all([
      api("/time-entries/all?status=submitted"),
      api("/absences/all?status=requested"),
      api("/change-requests/all?status=open"),
      api("/reopen-requests/pending"),
      api("/users"),
    ]);
    pendingEntries = e;
    pendingAbsences = a;
    changeRequests = c;
    pendingReopens = r;
    users = u;
  }
  load();
  loadChart();

  async function approveReopen(id) {
    await api(`/reopen-requests/${id}/approve`, {
      method: "POST",
      body: {},
    });
    toast($t("Approved."), "ok");
    load();
  }
  async function rejectReopen(id) {
    const reason = await confirmDialog(
      $t("Reject?"),
      $t("Reject this request?"),
      { danger: true, confirm: $t("Reject"), reason: true },
    );
    if (!reason) return;
    await api(`/reopen-requests/${id}/reject`, {
      method: "POST",
      body: { reason },
    });
    load();
  }

  function userName(uid) {
    const u = users.find((x) => x.id === uid);
    return u ? `${u.first_name} ${u.last_name}` : `#${uid}`;
  }
  function userInitials(uid) {
    const u = users.find((x) => x.id === uid);
    return u
      ? ((u.first_name?.[0] || "") + (u.last_name?.[0] || "")).toUpperCase()
      : "?";
  }

  async function approveEntry(id) {
    await api(`/time-entries/${id}/approve`, { method: "POST" });
    toast($t("Approved."), "ok");
    load();
  }
  async function rejectEntry(id) {
    const reason = await confirmDialog(
      $t("Reject?"),
      $t("Reject this entry?"),
      {
        danger: true,
        confirm: $t("Reject"),
        reason: true,
      },
    );
    if (!reason) return;
    await api(`/time-entries/${id}/reject`, {
      method: "POST",
      body: { reason },
    });
    load();
  }
  async function batchApprove() {
    const ids = pendingEntries.map((e) => e.id);
    await api("/time-entries/batch-approve", { method: "POST", body: { ids } });
    toast($t("All approved."), "ok");
    load();
  }
  async function approveAbsence(id) {
    await api(`/absences/${id}/approve`, { method: "POST" });
    toast($t("Approved."), "ok");
    load();
  }
  async function rejectAbsence(id) {
    const reason = await confirmDialog(
      $t("Reject?"),
      $t("Reject this request?"),
      {
        danger: true,
        confirm: $t("Reject"),
        reason: true,
      },
    );
    if (!reason) return;
    await api(`/absences/${id}/reject`, {
      method: "POST",
      body: { reason },
    });
    load();
  }
  async function approveCR(id) {
    await api(`/change-requests/${id}/approve`, { method: "POST" });
    toast($t("Approved."), "ok");
    load();
  }
  async function rejectCR(id) {
    const reason = await confirmDialog(
      $t("Reject?"),
      $t("Reject this change request?"),
      {
        danger: true,
        confirm: $t("Reject"),
        reason: true,
      },
    );
    if (!reason) return;
    await api(`/change-requests/${id}/reject`, {
      method: "POST",
      body: { reason },
    });
    load();
  }
</script>

<div class="top-bar">
  <div class="top-bar-title">
    <h1>{$t("Dashboard")}</h1>
    <div class="top-bar-subtitle">
      {$t("Approve timesheets & manage requests")}
    </div>
  </div>
</div>

<div class="content-area">
  <!-- Stats -->
  <div class="stat-cards">
    <div class="kz-card stat-card">
      <div class="stat-card-label">{$t("Pending Timesheets")}</div>
      <div class="stat-card-value accent tab-num">{pendingEntries.length}</div>
    </div>
    <div class="kz-card stat-card">
      <div class="stat-card-label">{$t("Absence Requests")}</div>
      <div class="stat-card-value tab-num">{pendingAbsences.length}</div>
    </div>
    <div class="kz-card stat-card">
      <div class="stat-card-label">{$t("Change Requests")}</div>
      <div class="stat-card-value tab-num">{changeRequests.length}</div>
    </div>
    <div class="kz-card stat-card">
      <div class="stat-card-label">{$t("Team Members")}</div>
      <div class="stat-card-value tab-num">{users.length}</div>
    </div>
  </div>

  <!-- Flextime balance chart -->
  <div class="kz-card" style="padding:16px 20px;margin-bottom:16px">
    <div
      style="display:flex;align-items:center;gap:10px;flex-wrap:wrap;margin-bottom:14px"
    >
      <Icon name="TrendingUp" size={15} sw={1.5} />
      <span style="font-size:14px;font-weight:600;flex:1"
        >{$t("Flextime balance")}</span
      >
      <!-- Quick-range buttons -->
      <div style="display:flex;gap:4px;flex-wrap:wrap">
        <button class="kz-btn kz-btn-sm" on:click={() => setRange(30)}
          >{$t("Last 30 days")}</button
        >
        <button class="kz-btn kz-btn-sm" on:click={() => setRange(90)}
          >{$t("Last 90 days")}</button
        >
        <button class="kz-btn kz-btn-sm" on:click={() => setRange(182)}
          >{$t("Last 6 months")}</button
        >
        <button class="kz-btn kz-btn-sm" on:click={() => setRange(365)}
          >{$t("Last year")}</button
        >
      </div>
      <!-- Custom date range -->
      <div style="display:flex;align-items:center;gap:4px">
        <input
          type="date"
          class="kz-input"
          style="font-size:12px;padding:3px 6px;height:28px"
          bind:value={chartFrom}
        />
        <span style="font-size:12px;color:var(--text-tertiary)">–</span>
        <input
          type="date"
          class="kz-input"
          style="font-size:12px;padding:3px 6px;height:28px"
          bind:value={chartTo}
        />
        <button class="kz-btn kz-btn-sm" on:click={loadChart}>
          <Icon name="Search" size={13} />{$t("Show")}
        </button>
      </div>
    </div>
    {#if chartLoading}
      <div
        style="text-align:center;padding:40px 0;font-size:13px;color:var(--text-tertiary)"
      >
        {$t("Loading...")}
      </div>
    {:else}
      <FlextimeChart data={chartData} />
    {/if}
  </div>

  <div style="display:grid;grid-template-columns:1fr 1fr;gap:16px">
    <!-- Timesheet approvals -->
    <div class="kz-card" style="overflow:hidden">
      <div class="card-header">
        <Icon name="FileText" size={15} sw={1.5} />
        <span class="card-header-title">{$t("Timesheet Approvals")}</span>
        {#if pendingEntries.length}
          <span class="kz-chip kz-chip-submitted" style="font-size:10.5px">
            {pendingEntries.length}
            {$t("pending")}
          </span>
          <button class="kz-btn kz-btn-sm" on:click={batchApprove}>
            <Icon name="Check" size={13} />{$t("Approve All")}
          </button>
        {/if}
      </div>
      {#each pendingEntries as e}
        <div
          style="padding:10px 16px;border-bottom:1px solid var(--border);display:flex;align-items:center;gap:10px"
        >
          <div class="avatar" style="width:30px;height:30px;font-size:11px">
            {userInitials(e.user_id)}
          </div>
          <div style="flex:1;min-width:0">
            <div style="font-size:13px;font-weight:500">
              {userName(e.user_id)}
            </div>
            <div
              class="tab-num"
              style="font-size:11.5px;color:var(--text-tertiary)"
            >
              {fmtDateShort(e.entry_date)} · {e.start_time.slice(
                0,
                5,
              )}–{e.end_time.slice(0, 5)}
            </div>
          </div>
          <div style="display:flex;gap:4px">
            <button
              class="kz-btn-icon-sm"
              style="color:var(--success-text);background:var(--success-soft)"
              title={$t("Approve")}
              on:click={() => approveEntry(e.id)}
            >
              <Icon name="Check" size={14} />
            </button>
            <button
              class="kz-btn-icon-sm"
              style="color:var(--danger-text);background:var(--danger-soft)"
              title={$t("Reject")}
              on:click={() => rejectEntry(e.id)}
            >
              <Icon name="X" size={14} />
            </button>
          </div>
        </div>
      {/each}
      {#if pendingEntries.length === 0}
        <div
          style="padding:32px;text-align:center;color:var(--text-tertiary);font-size:13px"
        >
          <Icon name="Check" size={24} sw={1.2} />
          <div style="margin-top:8px">{$t("All caught up!")}</div>
        </div>
      {/if}
    </div>

    <!-- Absence requests -->
    <div class="kz-card" style="overflow:hidden">
      <div class="card-header">
        <Icon name="Plane" size={15} sw={1.5} />
        <span class="card-header-title">{$t("Absence Requests")}</span>
        {#if pendingAbsences.length}
          <span class="kz-chip kz-chip-pending" style="font-size:10.5px">
            {pendingAbsences.length}
            {$t("pending")}
          </span>
        {/if}
      </div>
      {#each pendingAbsences as a}
        <div
          style="padding:10px 16px;border-bottom:1px solid var(--border);display:flex;align-items:center;gap:10px"
        >
          <div class="avatar" style="width:30px;height:30px;font-size:11px">
            {userInitials(a.user_id)}
          </div>
          <div style="flex:1;min-width:0">
            <div style="font-size:13px;font-weight:500">
              {userName(a.user_id)}
            </div>
            <div
              class="tab-num"
              style="font-size:11.5px;color:var(--text-tertiary)"
            >
              {absenceKindLabel(a.kind)} · {fmtDateShort(a.start_date)} – {fmtDateShort(
                a.end_date,
              )}
            </div>
          </div>
          <div style="display:flex;gap:4px">
            <button
              class="kz-btn-icon-sm"
              style="color:var(--success-text);background:var(--success-soft)"
              on:click={() => approveAbsence(a.id)}
            >
              <Icon name="Check" size={14} />
            </button>
            <button
              class="kz-btn-icon-sm"
              style="color:var(--danger-text);background:var(--danger-soft)"
              on:click={() => rejectAbsence(a.id)}
            >
              <Icon name="X" size={14} />
            </button>
          </div>
        </div>
      {/each}
      {#if pendingAbsences.length === 0}
        <div
          style="padding:32px;text-align:center;color:var(--text-tertiary);font-size:13px"
        >
          <Icon name="Plane" size={24} sw={1.2} />
          <div style="margin-top:8px">{$t("No pending requests")}</div>
        </div>
      {/if}
    </div>
  </div>

  <!-- Reopen requests (week-level) -->
  {#if pendingReopens.length > 0}
    <div class="kz-card" style="overflow:hidden;margin-top:16px">
      <div class="card-header">
        <Icon name="Edit" size={15} sw={1.5} />
        <span class="card-header-title">{$t("Week reopen requests")}</span>
        <span class="kz-chip kz-chip-pending" style="font-size:10.5px">
          {pendingReopens.length}
          {$t("open")}
        </span>
      </div>
      {#each pendingReopens as r}
        <div
          style="padding:10px 16px;border-bottom:1px solid var(--border);display:flex;align-items:center;gap:10px"
        >
          <div class="avatar" style="width:30px;height:30px;font-size:11px">
            {userInitials(r.user_id)}
          </div>
          <div style="flex:1;min-width:0">
            <div style="font-size:13px;font-weight:500">
              {userName(r.user_id)}
            </div>
            <div
              class="tab-num"
              style="font-size:11.5px;color:var(--text-tertiary)"
            >
              {$t("wants to edit week of {date}", {
                date: fmtDateShort(r.week_start),
              })}
            </div>
          </div>
          <div style="display:flex;gap:4px">
            <button
              class="kz-btn-icon-sm"
              style="color:var(--success-text);background:var(--success-soft)"
              title={$t("Approve")}
              on:click={() => approveReopen(r.id)}
            >
              <Icon name="Check" size={14} />
            </button>
            <button
              class="kz-btn-icon-sm"
              style="color:var(--danger-text);background:var(--danger-soft)"
              title={$t("Reject")}
              on:click={() => rejectReopen(r.id)}
            >
              <Icon name="X" size={14} />
            </button>
          </div>
        </div>
      {/each}
    </div>
  {/if}

  <!-- Change requests -->
  {#if changeRequests.length > 0}
    <div class="kz-card" style="overflow:hidden;margin-top:16px">
      <div class="card-header">
        <Icon name="Edit" size={15} sw={1.5} />
        <span class="card-header-title">{$t("Change Requests")}</span>
        <span class="kz-chip kz-chip-pending" style="font-size:10.5px">
          {changeRequests.length}
          {$t("open")}
        </span>
      </div>
      <table class="kz-table">
        <thead>
          <tr>
            <th>{$t("Employee")}</th>
            <th>{$t("Date")}</th>
            <th>{$t("Request")}</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each changeRequests as cr}
            <tr>
              <td style="font-weight:500">{userName(cr.user_id)}</td>
              <td class="tab-num">{fmtDate(cr.created_at)}</td>
              <td
                style="max-width:300px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap"
              >
                {cr.reason || "–"}
              </td>
              <td style="text-align:right">
                <div style="display:flex;gap:4px;justify-content:flex-end">
                  <button
                    class="kz-btn-icon-sm"
                    style="color:var(--success-text);background:var(--success-soft)"
                    on:click={() => approveCR(cr.id)}
                  >
                    <Icon name="Check" size={14} />
                  </button>
                  <button
                    class="kz-btn-icon-sm"
                    style="color:var(--danger-text);background:var(--danger-soft)"
                    on:click={() => rejectCR(cr.id)}
                  >
                    <Icon name="X" size={14} />
                  </button>
                </div>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</div>
