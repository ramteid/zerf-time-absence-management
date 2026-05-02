/* KitaZeit — Absences, Calendar, Account screens */

// ── Absences Screen (Desktop) ──
const AbsencesDesktop = () => {
  const [showReq, setShowReq] = React.useState(false);
  const absences = [
    { id: 1, type: 'Vacation', from: 'Dec 23', to: 'Jan 3', days: 8, status: 'approved' },
    { id: 2, type: 'Sick Leave', from: 'Nov 11', to: 'Nov 12', days: 2, status: 'approved' },
    { id: 3, type: 'Vacation', from: 'Feb 17', to: 'Feb 21', days: 5, status: 'pending' },
    { id: 4, type: 'Fortbildung', from: 'Mar 10', to: 'Mar 11', days: 2, status: 'draft' },
  ];
  const balance = { total: 30, used: 15, pending: 5, remaining: 10 };

  return (
    <div className="kz" style={{ display: 'flex', height: '100%', background: 'var(--bg-canvas)' }}>
      <Sidebar active="absences" isLead={true}/>
      <div style={{ flex: 1, display: 'flex', flexDirection: 'column', minWidth: 0 }}>
        <TopBar title="Absences" subtitle="Vacation, sick leave & training days">
          <button className="kz-btn kz-btn-primary" onClick={() => setShowReq(true)}>
            <Icons.Plus size={14}/>Request Absence
          </button>
        </TopBar>

        <div style={{ flex: 1, padding: '20px 28px', overflow: 'auto' }} className="kz-scroll">
          {/* Balance cards */}
          <div style={{ display: 'flex', gap: 12, marginBottom: 20 }}>
            <StatCard label="Total Days" value={balance.total}/>
            <StatCard label="Used" value={balance.used}/>
            <StatCard label="Pending" value={balance.pending} sub="awaiting approval"/>
            <StatCard label="Remaining" value={balance.remaining} accent/>
          </div>

          {/* Absences table */}
          <div className="kz-card" style={{ overflow: 'hidden' }}>
            <div style={{ padding: '12px 16px', borderBottom: '1px solid var(--border)', fontSize: 14, fontWeight: 600 }}>Absence History</div>
            <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: 13 }}>
              <thead>
                <tr style={{ borderBottom: '1px solid var(--border)', background: 'var(--bg-subtle)' }}>
                  {['Type', 'From', 'To', 'Days', 'Status', ''].map(h => (
                    <th key={h} style={{ padding: '8px 16px', textAlign: 'left', fontSize: 11.5, fontWeight: 500, color: 'var(--text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.04em' }}>{h}</th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {absences.map(a => (
                  <tr key={a.id} style={{ borderBottom: '1px solid var(--border)' }}>
                    <td style={{ padding: '10px 16px', fontWeight: 500 }}>{a.type}</td>
                    <td style={{ padding: '10px 16px' }} className="tab-num">{a.from}</td>
                    <td style={{ padding: '10px 16px' }} className="tab-num">{a.to}</td>
                    <td style={{ padding: '10px 16px' }} className="tab-num">{a.days}</td>
                    <td style={{ padding: '10px 16px' }}><StatusChip status={a.status}/></td>
                    <td style={{ padding: '10px 16px', textAlign: 'right' }}>
                      <button className="kz-btn kz-btn-ghost kz-btn-sm"><Icons.MoreH size={14}/></button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      </div>

      {/* Request dialog */}
      <Dialog open={showReq} onClose={() => setShowReq(false)} title="Request Absence">
        <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
          <div>
            <label className="kz-label">Type</label>
            <select className="kz-select"><option>Vacation</option><option>Sick Leave</option><option>Fortbildung</option><option>Other</option></select>
          </div>
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
            <div><label className="kz-label">From</label><input className="kz-input" type="date"/></div>
            <div><label className="kz-label">To</label><input className="kz-input" type="date"/></div>
          </div>
          <div><label className="kz-label">Notes (optional)</label><textarea className="kz-input" style={{ height: 72, padding: 10, resize: 'none' }}/></div>
          <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end', paddingTop: 4 }}>
            <button className="kz-btn" onClick={() => setShowReq(false)}>Cancel</button>
            <button className="kz-btn kz-btn-primary">Submit Request</button>
          </div>
        </div>
      </Dialog>
    </div>
  );
};

// ── Calendar Screen (Desktop) ──
const CalendarDesktop = () => {
  const today = 18; // Dec 18
  const daysInMonth = 31;
  const startDay = 6; // Dec 1 = Sunday → offset 6 (Mon-start)
  const monthDays = Array.from({ length: daysInMonth }, (_, i) => i + 1);
  
  // Some events
  const events = {
    16: [{ label: 'Gruppenarbeit', color: 'var(--cat-gruppe)' }],
    17: [{ label: 'Elterngespräch', color: 'var(--cat-elterngespraech)' }],
    18: [{ label: 'Teambesprechung', color: 'var(--cat-team)' }, { label: 'Gruppenarbeit', color: 'var(--cat-gruppe)' }],
    19: [{ label: 'Fortbildung', color: 'var(--cat-fortbildung)' }],
    20: [{ label: 'Gruppenarbeit', color: 'var(--cat-gruppe)' }],
    23: [{ label: 'Vacation', color: 'var(--success)' }],
    24: [{ label: 'Vacation', color: 'var(--success)' }],
    25: [{ label: 'Holiday', color: 'var(--danger)' }],
    26: [{ label: 'Holiday', color: 'var(--danger)' }],
  };

  return (
    <div className="kz" style={{ display: 'flex', height: '100%', background: 'var(--bg-canvas)' }}>
      <Sidebar active="calendar" isLead={true}/>
      <div style={{ flex: 1, display: 'flex', flexDirection: 'column', minWidth: 0 }}>
        <TopBar title="Calendar" subtitle="December 2024">
          <WeekNav weekLabel="December 2024"/>
        </TopBar>

        <div style={{ flex: 1, padding: '20px 28px', overflow: 'auto' }} className="kz-scroll">
          <div className="kz-card" style={{ padding: 16 }}>
            {/* Day headers */}
            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(7, 1fr)', gap: 1, marginBottom: 8 }}>
              {['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'].map(d => (
                <div key={d} style={{ textAlign: 'center', fontSize: 11, fontWeight: 500, color: 'var(--text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.04em', padding: '4px 0' }}>{d}</div>
              ))}
            </div>

            {/* Calendar grid */}
            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(7, 1fr)', gap: 1 }}>
              {/* Empty cells for offset */}
              {Array.from({ length: startDay }, (_, i) => (
                <div key={`e${i}`} style={{ minHeight: 80 }}/>
              ))}
              {monthDays.map(d => {
                const isToday = d === today;
                const evts = events[d] || [];
                const isWeekend = ((startDay + d - 1) % 7) >= 5;
                return (
                  <div key={d} style={{
                    minHeight: 80, padding: '4px 6px', borderRadius: 'var(--radius-sm)',
                    background: isToday ? 'var(--accent-soft)' : isWeekend ? 'var(--bg-muted)' : 'transparent',
                    border: isToday ? '1px solid var(--accent)' : '1px solid transparent',
                  }}>
                    <div className="tab-num" style={{
                      fontSize: 12, fontWeight: isToday ? 700 : 400,
                      color: isToday ? 'var(--accent-text)' : isWeekend ? 'var(--text-tertiary)' : 'var(--text-secondary)',
                      marginBottom: 4,
                    }}>{d}</div>
                    {evts.map((ev, i) => (
                      <div key={i} style={{
                        fontSize: 10, padding: '2px 5px', borderRadius: 3,
                        background: ev.color, color: '#fff', marginBottom: 2,
                        whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis',
                        fontWeight: 500, opacity: 0.9,
                      }}>{ev.label}</div>
                    ))}
                  </div>
                );
              })}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};

// ── Account Screen (Desktop) ──
const AccountDesktop = () => {
  return (
    <div className="kz" style={{ display: 'flex', height: '100%', background: 'var(--bg-canvas)' }}>
      <Sidebar active="account" isLead={true}/>
      <div style={{ flex: 1, display: 'flex', flexDirection: 'column', minWidth: 0 }}>
        <TopBar title="Account" subtitle="Your profile & preferences"/>

        <div style={{ flex: 1, padding: '20px 28px', overflow: 'auto', maxWidth: 640 }} className="kz-scroll">
          {/* Profile card */}
          <div className="kz-card" style={{ padding: 20, marginBottom: 16 }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 16, marginBottom: 20 }}>
              <Avatar initials={ME.avatar} size={56} bg="var(--accent-soft)" color="var(--accent-text)"/>
              <div>
                <div style={{ fontSize: 18, fontWeight: 600 }}>{ME.name}</div>
                <div style={{ fontSize: 13, color: 'var(--text-tertiary)' }}>{ME.role} · {ME.group}</div>
              </div>
            </div>
            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 14 }}>
              <div><label className="kz-label">Email</label><input className="kz-input" defaultValue="anna.mueller@kita-sonnenschein.de" readOnly style={{ color: 'var(--text-secondary)' }}/></div>
              <div><label className="kz-label">Phone</label><input className="kz-input" defaultValue="+49 170 1234567"/></div>
              <div><label className="kz-label">Contract Hours</label><input className="kz-input" defaultValue="32h / week" readOnly style={{ color: 'var(--text-secondary)' }}/></div>
              <div><label className="kz-label">Group</label><input className="kz-input" defaultValue={ME.group} readOnly style={{ color: 'var(--text-secondary)' }}/></div>
            </div>
          </div>

          {/* Preferences */}
          <div className="kz-card" style={{ padding: 20 }}>
            <div style={{ fontSize: 14, fontWeight: 600, marginBottom: 14 }}>Preferences</div>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
                <div>
                  <div style={{ fontSize: 13, fontWeight: 500 }}>Email Notifications</div>
                  <div style={{ fontSize: 12, color: 'var(--text-tertiary)' }}>Get notified when your week is approved</div>
                </div>
                <ToggleSwitch defaultOn/>
              </div>
              <div style={{ height: 1, background: 'var(--border)' }}/>
              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
                <div>
                  <div style={{ fontSize: 13, fontWeight: 500 }}>Weekly Reminder</div>
                  <div style={{ fontSize: 12, color: 'var(--text-tertiary)' }}>Remind me to submit on Fridays at 14:00</div>
                </div>
                <ToggleSwitch defaultOn/>
              </div>
              <div style={{ height: 1, background: 'var(--border)' }}/>
              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
                <div>
                  <div style={{ fontSize: 13, fontWeight: 500 }}>Dark Mode</div>
                  <div style={{ fontSize: 12, color: 'var(--text-tertiary)' }}>Switch to dark interface</div>
                </div>
                <ToggleSwitch/>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};

const ToggleSwitch = ({ defaultOn = false }) => {
  const [on, setOn] = React.useState(defaultOn);
  return (
    <div onClick={() => setOn(!on)} style={{
      width: 40, height: 22, borderRadius: 11, padding: 2, cursor: 'pointer',
      background: on ? 'var(--accent)' : 'var(--border-strong)',
      transition: 'background .15s', flexShrink: 0,
    }}>
      <div style={{
        width: 18, height: 18, borderRadius: '50%', background: '#fff',
        boxShadow: 'var(--shadow-sm)',
        transform: on ? 'translateX(18px)' : 'translateX(0)',
        transition: 'transform .15s',
      }}/>
    </div>
  );
};

// ── Absences Mobile ──
const AbsencesMobile = () => {
  const absences = [
    { type: 'Vacation', from: 'Dec 23', to: 'Jan 3', days: 8, status: 'approved' },
    { type: 'Sick Leave', from: 'Nov 11', to: 'Nov 12', days: 2, status: 'approved' },
    { type: 'Vacation', from: 'Feb 17', to: 'Feb 21', days: 5, status: 'pending' },
  ];
  return (
    <div className="kz" style={{ height: '100%', background: 'var(--bg-canvas)', display: 'flex', flexDirection: 'column' }}>
      <div style={{ background: 'var(--nav-bg)', color: '#fff', padding: '14px 16px' }}>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
          <span style={{ fontWeight: 600, fontSize: 17 }}>Absences</span>
          <button className="kz-btn kz-btn-primary kz-btn-sm" style={{ background: 'var(--accent)', border: 'none' }}><Icons.Plus size={13}/>Request</button>
        </div>
      </div>
      
      {/* Balance */}
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 8, padding: '12px 16px' }}>
        <div className="kz-card" style={{ padding: '10px 12px', textAlign: 'center' }}>
          <div style={{ fontSize: 20, fontWeight: 700, color: 'var(--accent)' }}>10</div>
          <div style={{ fontSize: 10.5, color: 'var(--text-tertiary)', fontWeight: 500, textTransform: 'uppercase' }}>Remaining</div>
        </div>
        <div className="kz-card" style={{ padding: '10px 12px', textAlign: 'center' }}>
          <div style={{ fontSize: 20, fontWeight: 700 }}>15</div>
          <div style={{ fontSize: 10.5, color: 'var(--text-tertiary)', fontWeight: 500, textTransform: 'uppercase' }}>Used</div>
        </div>
      </div>

      <div style={{ flex: 1, overflow: 'auto', padding: '0 16px 16px' }} className="kz-scroll">
        {absences.map((a, i) => (
          <div key={i} className="kz-card" style={{ padding: '12px 14px', marginBottom: 8, display: 'flex', alignItems: 'center', gap: 12 }}>
            <div style={{ flex: 1 }}>
              <div style={{ fontSize: 13, fontWeight: 500, marginBottom: 2 }}>{a.type}</div>
              <div style={{ fontSize: 12, color: 'var(--text-tertiary)' }} className="tab-num">{a.from} – {a.to} · {a.days}d</div>
            </div>
            <StatusChip status={a.status}/>
          </div>
        ))}
      </div>
      <MobileNav active="absences"/>
    </div>
  );
};

Object.assign(window, { AbsencesDesktop, CalendarDesktop, AccountDesktop, AbsencesMobile, ToggleSwitch });
