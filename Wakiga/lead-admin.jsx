/* KitaZeit — Lead Dashboard + Admin screens */

// ── Lead Dashboard ──
const LeadDashboard = () => {
  const pending = [
    { emp: EMPLOYEES[1], week: 'Dec 16–20', hours: 38.5, status: 'submitted' },
    { emp: EMPLOYEES[2], week: 'Dec 16–20', hours: 24.0, status: 'submitted' },
    { emp: EMPLOYEES[4], week: 'Dec 16–20', hours: 29.5, status: 'submitted' },
    { emp: EMPLOYEES[3], week: 'Dec 9–13', hours: 41.0, status: 'submitted' },
  ];
  const absReq = [
    { emp: EMPLOYEES[5], type: 'Vacation', from: 'Jan 6', to: 'Jan 10', days: 5 },
    { emp: EMPLOYEES[2], type: 'Fortbildung', from: 'Jan 20', to: 'Jan 21', days: 2 },
  ];
  const [items, setItems] = React.useState(pending);
  const [absItems, setAbsItems] = React.useState(absReq);

  const approve = (i) => {
    setItems(prev => prev.map((it, j) => j === i ? { ...it, status: 'approved' } : it));
  };
  const reject = (i) => {
    setItems(prev => prev.map((it, j) => j === i ? { ...it, status: 'rejected' } : it));
  };

  return (
    <div className="kz" style={{ display: 'flex', height: '100%', background: 'var(--bg-canvas)' }}>
      <Sidebar active="dashboard" isLead={true}/>
      <div style={{ flex: 1, display: 'flex', flexDirection: 'column', minWidth: 0 }}>
        <TopBar title="Dashboard" subtitle="Approve timesheets & manage requests"/>

        <div style={{ flex: 1, padding: '20px 28px', overflow: 'auto' }} className="kz-scroll">
          {/* Stats */}
          <div style={{ display: 'flex', gap: 12, marginBottom: 20 }}>
            <StatCard label="Pending Timesheets" value={items.filter(i => i.status === 'submitted').length} accent/>
            <StatCard label="Absence Requests" value={absItems.length}/>
            <StatCard label="Team Members" value={EMPLOYEES.length}/>
            <StatCard label="Avg Hours / Week" value="33.2h"/>
          </div>

          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 16 }}>
            {/* Timesheets to approve */}
            <div className="kz-card" style={{ overflow: 'hidden' }}>
              <div style={{ padding: '12px 16px', borderBottom: '1px solid var(--border)', display: 'flex', alignItems: 'center', gap: 8 }}>
                <Icons.FileText size={15} style={{ color: 'var(--text-tertiary)' }}/>
                <span style={{ fontSize: 14, fontWeight: 600, flex: 1 }}>Timesheet Approvals</span>
                <span className="kz-chip kz-chip-submitted" style={{ fontSize: 10.5 }}>{items.filter(i => i.status === 'submitted').length} pending</span>
              </div>
              <div>
                {items.map((it, i) => (
                  <div key={i} style={{ padding: '10px 16px', borderBottom: '1px solid var(--border)', display: 'flex', alignItems: 'center', gap: 10 }}>
                    <Avatar initials={it.emp.avatar} size={30}/>
                    <div style={{ flex: 1, minWidth: 0 }}>
                      <div style={{ fontSize: 13, fontWeight: 500 }}>{it.emp.name}</div>
                      <div style={{ fontSize: 11.5, color: 'var(--text-tertiary)' }} className="tab-num">{it.week} · {it.hours}h</div>
                    </div>
                    {it.status === 'submitted' ? (
                      <div style={{ display: 'flex', gap: 4 }}>
                        <button className="kz-btn kz-btn-sm kz-btn-icon-sm" title="Approve" onClick={() => approve(i)}
                          style={{ color: 'var(--success-text)', background: 'var(--success-soft)', border: 'none' }}>
                          <Icons.Check size={14}/>
                        </button>
                        <button className="kz-btn kz-btn-sm kz-btn-icon-sm" title="Reject" onClick={() => reject(i)}
                          style={{ color: 'var(--danger-text)', background: 'var(--danger-soft)', border: 'none' }}>
                          <Icons.X size={14}/>
                        </button>
                      </div>
                    ) : (
                      <StatusChip status={it.status}/>
                    )}
                  </div>
                ))}
              </div>
            </div>

            {/* Absence requests */}
            <div className="kz-card" style={{ overflow: 'hidden' }}>
              <div style={{ padding: '12px 16px', borderBottom: '1px solid var(--border)', display: 'flex', alignItems: 'center', gap: 8 }}>
                <Icons.Plane size={15} style={{ color: 'var(--text-tertiary)' }}/>
                <span style={{ fontSize: 14, fontWeight: 600, flex: 1 }}>Absence Requests</span>
                <span className="kz-chip kz-chip-pending" style={{ fontSize: 10.5 }}>{absItems.length} pending</span>
              </div>
              {absItems.map((it, i) => (
                <div key={i} style={{ padding: '10px 16px', borderBottom: '1px solid var(--border)', display: 'flex', alignItems: 'center', gap: 10 }}>
                  <Avatar initials={it.emp.avatar} size={30}/>
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <div style={{ fontSize: 13, fontWeight: 500 }}>{it.emp.name}</div>
                    <div style={{ fontSize: 11.5, color: 'var(--text-tertiary)' }} className="tab-num">{it.type} · {it.from} – {it.to} ({it.days}d)</div>
                  </div>
                  <div style={{ display: 'flex', gap: 4 }}>
                    <button className="kz-btn kz-btn-sm kz-btn-icon-sm" style={{ color: 'var(--success-text)', background: 'var(--success-soft)', border: 'none' }}>
                      <Icons.Check size={14}/>
                    </button>
                    <button className="kz-btn kz-btn-sm kz-btn-icon-sm" style={{ color: 'var(--danger-text)', background: 'var(--danger-soft)', border: 'none' }}>
                      <Icons.X size={14}/>
                    </button>
                  </div>
                </div>
              ))}
              {absItems.length === 0 && <EmptyState icon={Icons.Plane} message="No pending requests"/>}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};

// ── Reports Screen ──
const ReportsDesktop = () => {
  const weekData = [
    { name: 'Anna Müller', hours: [6.5, 7.0, 6.5, 6.0, 5.5], total: 31.5, target: 32 },
    { name: 'Thomas Weber', hours: [8.0, 7.0, 6.5, 7.5, 8.0], total: 37.0, target: 40 },
    { name: 'Lena Schmidt', hours: [5.0, 5.5, 5.0, 5.0, 4.5], total: 25.0, target: 25 },
    { name: 'Markus Braun', hours: [8.0, 8.5, 8.0, 7.0, 8.5], total: 40.0, target: 40 },
    { name: 'Sofia Kaya', hours: [6.0, 6.5, 6.0, 6.0, 5.5], total: 30.0, target: 30 },
    { name: 'Jan Peters', hours: [4.0, 4.0, 4.0, 4.0, 4.0], total: 20.0, target: 20 },
  ];

  return (
    <div className="kz" style={{ display: 'flex', height: '100%', background: 'var(--bg-canvas)' }}>
      <Sidebar active="reports" isLead={true}/>
      <div style={{ flex: 1, display: 'flex', flexDirection: 'column', minWidth: 0 }}>
        <TopBar title="Reports" subtitle="Team hours overview">
          <WeekNav/>
          <button className="kz-btn"><Icons.Download size={14}/>Export</button>
        </TopBar>

        <div style={{ flex: 1, padding: '20px 28px', overflow: 'auto' }} className="kz-scroll">
          <div className="kz-card" style={{ overflow: 'hidden' }}>
            <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: 13 }}>
              <thead>
                <tr style={{ borderBottom: '1px solid var(--border)', background: 'var(--bg-subtle)' }}>
                  <th style={{ padding: '10px 16px', textAlign: 'left', fontSize: 11.5, fontWeight: 500, color: 'var(--text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.04em' }}>Employee</th>
                  {DAYS.map(d => <th key={d} style={{ padding: '10px 12px', textAlign: 'center', fontSize: 11.5, fontWeight: 500, color: 'var(--text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.04em' }}>{d}</th>)}
                  <th style={{ padding: '10px 16px', textAlign: 'right', fontSize: 11.5, fontWeight: 500, color: 'var(--text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.04em' }}>Total</th>
                  <th style={{ padding: '10px 16px', textAlign: 'right', fontSize: 11.5, fontWeight: 500, color: 'var(--text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.04em' }}>Target</th>
                  <th style={{ padding: '10px 16px', textAlign: 'right', fontSize: 11.5, fontWeight: 500, color: 'var(--text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.04em' }}>Diff</th>
                </tr>
              </thead>
              <tbody>
                {weekData.map((row, i) => {
                  const diff = row.total - row.target;
                  return (
                    <tr key={i} style={{ borderBottom: '1px solid var(--border)' }}>
                      <td style={{ padding: '10px 16px', fontWeight: 500 }}>{row.name}</td>
                      {row.hours.map((h, j) => (
                        <td key={j} className="tab-num" style={{ padding: '10px 12px', textAlign: 'center', color: 'var(--text-secondary)' }}>{h.toFixed(1)}</td>
                      ))}
                      <td className="tab-num" style={{ padding: '10px 16px', textAlign: 'right', fontWeight: 600 }}>{row.total.toFixed(1)}h</td>
                      <td className="tab-num" style={{ padding: '10px 16px', textAlign: 'right', color: 'var(--text-tertiary)' }}>{row.target}h</td>
                      <td className="tab-num" style={{
                        padding: '10px 16px', textAlign: 'right', fontWeight: 500,
                        color: diff > 0 ? 'var(--warning-text)' : diff < 0 ? 'var(--danger-text)' : 'var(--success-text)',
                      }}>{diff > 0 ? '+' : ''}{diff.toFixed(1)}h</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        </div>
      </div>
    </div>
  );
};

// ── Admin Settings ──
const AdminSettings = () => {
  return (
    <div className="kz" style={{ display: 'flex', height: '100%', background: 'var(--bg-canvas)' }}>
      <Sidebar active="settings" isLead={true}/>
      <div style={{ flex: 1, display: 'flex', flexDirection: 'column', minWidth: 0 }}>
        <TopBar title="Settings" subtitle="Manage your Kita's configuration"/>

        <div style={{ flex: 1, padding: '20px 28px', overflow: 'auto', maxWidth: 760 }} className="kz-scroll">
          {/* Team members */}
          <div className="kz-card" style={{ overflow: 'hidden', marginBottom: 16 }}>
            <div style={{ padding: '12px 16px', borderBottom: '1px solid var(--border)', display: 'flex', alignItems: 'center' }}>
              <span style={{ fontSize: 14, fontWeight: 600, flex: 1 }}>Team Members</span>
              <button className="kz-btn kz-btn-sm"><Icons.Plus size={13}/>Add Member</button>
            </div>
            {EMPLOYEES.map((emp, i) => (
              <div key={emp.id} style={{ padding: '10px 16px', borderBottom: i < EMPLOYEES.length - 1 ? '1px solid var(--border)' : 'none', display: 'flex', alignItems: 'center', gap: 12 }}>
                <Avatar initials={emp.avatar} size={32}/>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ fontSize: 13, fontWeight: 500 }}>{emp.name}</div>
                  <div style={{ fontSize: 11.5, color: 'var(--text-tertiary)' }}>{emp.role} · {emp.group} · {emp.hours}h/week</div>
                </div>
                <button className="kz-btn kz-btn-ghost kz-btn-sm"><Icons.Edit size={13}/></button>
              </div>
            ))}
          </div>

          {/* Categories */}
          <div className="kz-card" style={{ overflow: 'hidden', marginBottom: 16 }}>
            <div style={{ padding: '12px 16px', borderBottom: '1px solid var(--border)', display: 'flex', alignItems: 'center' }}>
              <span style={{ fontSize: 14, fontWeight: 600, flex: 1 }}>Time Categories</span>
              <button className="kz-btn kz-btn-sm"><Icons.Plus size={13}/>Add Category</button>
            </div>
            {CATEGORIES.map((cat, i) => (
              <div key={cat.id} style={{ padding: '10px 16px', borderBottom: i < CATEGORIES.length - 1 ? '1px solid var(--border)' : 'none', display: 'flex', alignItems: 'center', gap: 10 }}>
                <span style={{ width: 10, height: 10, borderRadius: '50%', background: cat.color }}/>
                <span style={{ fontSize: 13, fontWeight: 500, flex: 1 }}>{cat.label}</span>
                <button className="kz-btn kz-btn-ghost kz-btn-sm"><Icons.Edit size={13}/></button>
              </div>
            ))}
          </div>

          {/* General settings */}
          <div className="kz-card" style={{ padding: 20 }}>
            <div style={{ fontSize: 14, fontWeight: 600, marginBottom: 14 }}>General</div>
            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 14 }}>
              <div>
                <label className="kz-label">Kita Name</label>
                <input className="kz-input" defaultValue="Kita Sonnenschein"/>
              </div>
              <div>
                <label className="kz-label">Week Starts On</label>
                <select className="kz-select"><option>Monday</option><option>Sunday</option></select>
              </div>
              <div>
                <label className="kz-label">Submission Deadline</label>
                <select className="kz-select"><option>Friday 16:00</option><option>Friday 18:00</option><option>Sunday 23:59</option></select>
              </div>
              <div>
                <label className="kz-label">Auto-approve Threshold</label>
                <select className="kz-select"><option>Off</option><option>Within ±1h of target</option><option>Within ±2h of target</option></select>
              </div>
            </div>
            <div style={{ marginTop: 16, display: 'flex', justifyContent: 'flex-end' }}>
              <button className="kz-btn kz-btn-primary">Save Changes</button>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};

Object.assign(window, { LeadDashboard, ReportsDesktop, AdminSettings });
