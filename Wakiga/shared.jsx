/* KitaZeit shared components */

// ── Data ──
const EMPLOYEES = [
  { id: 1, name: 'Anna Müller', role: 'Erzieherin', group: 'Sonnenkinder', avatar: 'AM', hours: 32 },
  { id: 2, name: 'Thomas Weber', role: 'Erzieher', group: 'Mondgruppe', avatar: 'TW', hours: 40 },
  { id: 3, name: 'Lena Schmidt', role: 'Kinderpflegerin', group: 'Sternschnuppen', avatar: 'LS', hours: 25 },
  { id: 4, name: 'Markus Braun', role: 'Erzieher', group: 'Sonnenkinder', avatar: 'MB', hours: 40 },
  { id: 5, name: 'Sofia Kaya', role: 'Erzieherin', group: 'Regenbogen', avatar: 'SK', hours: 30 },
  { id: 6, name: 'Jan Peters', role: 'Praktikant', group: 'Mondgruppe', avatar: 'JP', hours: 20 },
];
const ME = EMPLOYEES[0];

const CATEGORIES = [
  { id: 'gruppe', label: 'Gruppenarbeit', color: 'var(--cat-gruppe)' },
  { id: 'vorbereitung', label: 'Vorbereitung', color: 'var(--cat-vorbereitung)' },
  { id: 'elterngespraech', label: 'Elterngespräch', color: 'var(--cat-elterngespraech)' },
  { id: 'team', label: 'Teambesprechung', color: 'var(--cat-team)' },
  { id: 'fortbildung', label: 'Fortbildung', color: 'var(--cat-fortbildung)' },
  { id: 'pause', label: 'Pause', color: 'var(--cat-pause)' },
];

const DAYS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri'];
const DAYS_FULL = ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday'];

// Generate a week of sample entries
function genWeekEntries() {
  const patterns = [
    // Mon
    [{ cat: 'gruppe', start: '07:30', end: '12:00' }, { cat: 'pause', start: '12:00', end: '12:30' }, { cat: 'vorbereitung', start: '12:30', end: '14:00' }],
    // Tue
    [{ cat: 'gruppe', start: '08:00', end: '11:30' }, { cat: 'elterngespraech', start: '11:30', end: '12:30' }, { cat: 'pause', start: '12:30', end: '13:00' }, { cat: 'gruppe', start: '13:00', end: '15:00' }],
    // Wed
    [{ cat: 'team', start: '08:00', end: '09:30' }, { cat: 'gruppe', start: '09:30', end: '12:30' }, { cat: 'pause', start: '12:30', end: '13:00' }, { cat: 'vorbereitung', start: '13:00', end: '14:30' }],
    // Thu
    [{ cat: 'fortbildung', start: '09:00', end: '12:00' }, { cat: 'pause', start: '12:00', end: '12:30' }, { cat: 'gruppe', start: '12:30', end: '15:30' }],
    // Fri
    [{ cat: 'gruppe', start: '07:30', end: '11:00' }, { cat: 'pause', start: '11:00', end: '11:30' }, { cat: 'vorbereitung', start: '11:30', end: '13:00' }],
  ];
  return patterns;
}

function timeToMin(t) { const [h,m] = t.split(':').map(Number); return h*60+m; }
function minToTime(m) { return `${String(Math.floor(m/60)).padStart(2,'0')}:${String(m%60).padStart(2,'0')}`; }
function duration(s,e) { return ((timeToMin(e)-timeToMin(s))/60).toFixed(1); }
function dayTotal(entries) { return entries.reduce((s,e) => s + (timeToMin(e.end)-timeToMin(e.start))/60, 0); }

// ── Components ──
const Avatar = ({ initials, size = 32, bg = 'var(--accent-soft)', color = 'var(--accent-text)' }) => (
  <div style={{ width: size, height: size, borderRadius: '50%', background: bg, color,
    display: 'flex', alignItems: 'center', justifyContent: 'center',
    fontSize: size * 0.38, fontWeight: 600, flexShrink: 0, letterSpacing: '-0.02em' }}>{initials}</div>
);

const StatusChip = ({ status }) => {
  const map = { draft: 'Draft', submitted: 'Submitted', approved: 'Approved', rejected: 'Rejected', pending: 'Pending' };
  return <span className={`kz-chip kz-chip-${status}`}>{map[status] || status}</span>;
};

const CatDot = ({ catId, size = 8 }) => {
  const cat = CATEGORIES.find(c => c.id === catId);
  return <span style={{ width: size, height: size, borderRadius: '50%', background: cat?.color || '#ccc', display: 'inline-block', flexShrink: 0 }}/>;
};

const CatBadge = ({ catId }) => {
  const cat = CATEGORIES.find(c => c.id === catId);
  return (
    <span style={{ display: 'inline-flex', alignItems: 'center', gap: 5, fontSize: 12, color: 'var(--text-secondary)' }}>
      <CatDot catId={catId} size={7}/>{cat?.label}
    </span>
  );
};

// Sidebar
const Sidebar = ({ active = 'time', onNav, width = 220, isLead = false }) => {
  const items = [
    { id: 'time', icon: 'Clock', label: 'Time Entry' },
    { id: 'absences', icon: 'Plane', label: 'Absences' },
    { id: 'calendar', icon: 'Calendar', label: 'Calendar' },
    { id: 'account', icon: 'User', label: 'Account' },
  ];
  const leadItems = [
    { id: 'dashboard', icon: 'Home', label: 'Dashboard' },
    { id: 'reports', icon: 'BarChart', label: 'Reports' },
    { id: 'team', icon: 'Users', label: 'Team' },
  ];
  const adminItems = [
    { id: 'settings', icon: 'Settings', label: 'Settings' },
  ];

  return (
    <div style={{ width, background: 'var(--nav-bg)', display: 'flex', flexDirection: 'column', height: '100%', flexShrink: 0 }}>
      {/* Logo */}
      <div style={{ padding: '20px 18px 12px', display: 'flex', alignItems: 'center', gap: 10 }}>
        <div style={{ width: 30, height: 30, borderRadius: 8, background: 'var(--accent)', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
          <Icons.Clock size={16} style={{ stroke: '#fff' }}/>
        </div>
        <span style={{ color: '#fff', fontWeight: 600, fontSize: 16, letterSpacing: '-0.02em' }}>KitaZeit</span>
      </div>

      <div style={{ padding: '8px 0', flex: 1, overflow: 'auto' }}>
        <div className="kz-nav-section">Employee</div>
        {items.map(it => {
          const Ic = Icons[it.icon];
          return (
            <div key={it.id} className={`kz-nav-item ${active === it.id ? 'active' : ''}`}
              onClick={() => onNav?.(it.id)}>
              <Ic size={17}/><span>{it.label}</span>
            </div>
          );
        })}

        {isLead && <>
          <div className="kz-nav-section" style={{ marginTop: 8 }}>Lead</div>
          {leadItems.map(it => {
            const Ic = Icons[it.icon];
            return (
              <div key={it.id} className={`kz-nav-item ${active === it.id ? 'active' : ''}`}
                onClick={() => onNav?.(it.id)}>
                <Ic size={17}/><span>{it.label}</span>
              </div>
            );
          })}
        </>}

        <div className="kz-nav-section" style={{ marginTop: 8 }}>Admin</div>
        {adminItems.map(it => {
          const Ic = Icons[it.icon];
          return (
            <div key={it.id} className={`kz-nav-item ${active === it.id ? 'active' : ''}`}
              onClick={() => onNav?.(it.id)}>
              <Ic size={17}/><span>{it.label}</span>
            </div>
          );
        })}
      </div>

      {/* User */}
      <div style={{ padding: '12px 10px', borderTop: '1px solid var(--nav-border)', display: 'flex', alignItems: 'center', gap: 10 }}>
        <Avatar initials={ME.avatar} size={30} bg="var(--nav-bg-active)" color="var(--nav-text-active)"/>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ fontSize: 13, fontWeight: 500, color: 'var(--nav-text-active)', whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>{ME.name}</div>
          <div style={{ fontSize: 11, color: 'var(--nav-text-muted)' }}>{ME.role}</div>
        </div>
      </div>
    </div>
  );
};

// Top header bar
const TopBar = ({ title, subtitle, children }) => (
  <div style={{ padding: '20px 28px 16px', display: 'flex', alignItems: 'flex-start', gap: 16, borderBottom: '1px solid var(--border)' }}>
    <div style={{ flex: 1 }}>
      <h1 style={{ margin: 0, fontSize: 20, fontWeight: 600, letterSpacing: '-0.02em', color: 'var(--text-primary)' }}>{title}</h1>
      {subtitle && <div style={{ fontSize: 13, color: 'var(--text-tertiary)', marginTop: 2 }}>{subtitle}</div>}
    </div>
    {children && <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>{children}</div>}
  </div>
);

// Week nav
const WeekNav = ({ weekLabel = 'Dec 16 – 20, 2024', onPrev, onNext }) => (
  <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
    <button className="kz-btn kz-btn-icon-sm kz-btn-ghost" onClick={onPrev}><Icons.ChevLeft size={16}/></button>
    <span style={{ fontSize: 13.5, fontWeight: 500, minWidth: 140, textAlign: 'center' }} className="tab-num">{weekLabel}</span>
    <button className="kz-btn kz-btn-icon-sm kz-btn-ghost" onClick={onNext}><Icons.ChevRight size={16}/></button>
  </div>
);

// Stat card
const StatCard = ({ label, value, sub, accent = false }) => (
  <div className="kz-card" style={{ padding: '14px 16px', flex: 1, minWidth: 120 }}>
    <div style={{ fontSize: 11.5, fontWeight: 500, color: 'var(--text-tertiary)', letterSpacing: '0.02em', textTransform: 'uppercase', marginBottom: 4 }}>{label}</div>
    <div style={{ fontSize: 22, fontWeight: 600, color: accent ? 'var(--accent)' : 'var(--text-primary)', letterSpacing: '-0.02em' }} className="tab-num">{value}</div>
    {sub && <div style={{ fontSize: 12, color: 'var(--text-tertiary)', marginTop: 2 }}>{sub}</div>}
  </div>
);

// Modal / Dialog
const Dialog = ({ open, onClose, title, width = 420, children }) => {
  if (!open) return null;
  return (
    <div style={{ position: 'absolute', inset: 0, background: 'rgba(20,22,26,0.35)', display: 'flex', alignItems: 'center', justifyContent: 'center', zIndex: 50 }}
      onClick={onClose}>
      <div className="kz-card" style={{ width, maxHeight: '80%', overflow: 'auto', padding: 0, boxShadow: 'var(--shadow-lg)' }}
        onClick={e => e.stopPropagation()}>
        <div style={{ padding: '16px 20px', borderBottom: '1px solid var(--border)', display: 'flex', alignItems: 'center' }}>
          <h3 style={{ margin: 0, fontSize: 15, fontWeight: 600, flex: 1 }}>{title}</h3>
          <button className="kz-btn kz-btn-icon-sm kz-btn-ghost" onClick={onClose}><Icons.X size={16}/></button>
        </div>
        <div style={{ padding: '16px 20px' }}>{children}</div>
      </div>
    </div>
  );
};

// Empty state
const EmptyState = ({ icon: Ic, message }) => (
  <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', padding: 40, color: 'var(--text-tertiary)' }}>
    <Ic size={32} style={{ strokeWidth: 1.2, marginBottom: 8 }}/>
    <div style={{ fontSize: 13 }}>{message}</div>
  </div>
);

Object.assign(window, {
  EMPLOYEES, ME, CATEGORIES, DAYS, DAYS_FULL, genWeekEntries,
  timeToMin, minToTime, duration, dayTotal,
  Avatar, StatusChip, CatDot, CatBadge, Sidebar, TopBar, WeekNav, StatCard, Dialog, EmptyState,
});
