/* KitaZeit — Time Entry Screen (Desktop + Mobile) */

// ── Desktop Time Entry ──
const TimeEntryDesktop = () => {
  const week = genWeekEntries();
  const [status, setStatus] = React.useState('draft');
  const [addOpen, setAddOpen] = React.useState(false);
  const [addDay, setAddDay] = React.useState(0);
  const [entries, setEntries] = React.useState(week);
  const [editEntry, setEditEntry] = React.useState(null);

  const weekTotal = entries.reduce((s, day) => s + dayTotal(day), 0);
  const dates = ['Dec 16', 'Dec 17', 'Dec 18', 'Dec 19', 'Dec 20'];

  const handleSubmit = () => setStatus('submitted');
  const handleAddEntry = (dayIdx, entry) => {
    const next = entries.map((d, i) => i === dayIdx ? [...d, entry] : d);
    setEntries(next);
    setAddOpen(false);
  };
  const handleDeleteEntry = (dayIdx, entryIdx) => {
    const next = entries.map((d, i) => i === dayIdx ? d.filter((_, j) => j !== entryIdx) : d);
    setEntries(next);
  };

  return (
    <div className="kz" style={{ display: 'flex', height: '100%', background: 'var(--bg-canvas)' }}>
      <Sidebar active="time" isLead={true}/>
      <div style={{ flex: 1, display: 'flex', flexDirection: 'column', minWidth: 0 }}>
        <TopBar title="Time Entry" subtitle={`Week 51 · ${ME.hours}h contract`}>
          <WeekNav/>
          {status === 'draft' && (
            <button className="kz-btn kz-btn-primary" onClick={handleSubmit}>
              <Icons.Send size={14}/>Submit Week
            </button>
          )}
          {status !== 'draft' && <StatusChip status={status}/>}
        </TopBar>

        {/* Summary strip */}
        <div style={{ padding: '16px 28px 0', display: 'flex', gap: 12 }}>
          <StatCard label="Logged" value={`${weekTotal.toFixed(1)}h`} sub={`of ${ME.hours}h target`} accent/>
          <StatCard label="Overtime" value={`${Math.max(0, weekTotal - ME.hours).toFixed(1)}h`} sub="this week"/>
          <StatCard label="Remaining" value={`${Math.max(0, ME.hours - weekTotal).toFixed(1)}h`} sub="to target"/>
          <StatCard label="Status" value={status.charAt(0).toUpperCase() + status.slice(1)}/>
        </div>

        {/* Week grid */}
        <div style={{ flex: 1, padding: '16px 28px 24px', overflow: 'auto' }} className="kz-scroll">
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(5, 1fr)', gap: 12, minWidth: 680 }}>
            {DAYS.map((day, di) => {
              const dayEntries = entries[di];
              const total = dayTotal(dayEntries);
              return (
                <div key={day} className="kz-card" style={{ padding: 0, overflow: 'hidden' }}>
                  {/* Day header */}
                  <div style={{ padding: '10px 14px', borderBottom: '1px solid var(--border)', display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
                    <div>
                      <div style={{ fontSize: 13, fontWeight: 600 }}>{DAYS_FULL[di]}</div>
                      <div style={{ fontSize: 11.5, color: 'var(--text-tertiary)' }} className="tab-num">{dates[di]}</div>
                    </div>
                    <div style={{ fontSize: 15, fontWeight: 600, color: total >= (ME.hours/5) ? 'var(--accent)' : 'var(--text-primary)' }} className="tab-num">
                      {total.toFixed(1)}h
                    </div>
                  </div>

                  {/* Entries */}
                  <div style={{ padding: '6px 8px', display: 'flex', flexDirection: 'column', gap: 4, minHeight: 120 }}>
                    {dayEntries.map((entry, ei) => {
                      const cat = CATEGORIES.find(c => c.id === entry.cat);
                      return (
                        <div key={ei} style={{
                          padding: '8px 10px', borderRadius: 'var(--radius-md)',
                          background: 'var(--bg-subtle)', border: '1px solid var(--border)',
                          cursor: 'pointer', transition: 'background .1s',
                        }}
                        onMouseEnter={e => e.currentTarget.style.background = 'var(--bg-muted)'}
                        onMouseLeave={e => e.currentTarget.style.background = 'var(--bg-subtle)'}
                        onClick={() => setEditEntry({ di, ei, ...entry })}>
                          <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 3 }}>
                            <span style={{ width: 6, height: 6, borderRadius: '50%', background: cat?.color, flexShrink: 0 }}/>
                            <span style={{ fontSize: 12, fontWeight: 500, flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{cat?.label}</span>
                          </div>
                          <div style={{ fontSize: 12, color: 'var(--text-tertiary)', display: 'flex', justifyContent: 'space-between' }} className="tab-num">
                            <span>{entry.start} – {entry.end}</span>
                            <span>{duration(entry.start, entry.end)}h</span>
                          </div>
                        </div>
                      );
                    })}
                  </div>

                  {/* Add button */}
                  {status === 'draft' && (
                    <div style={{ padding: '6px 8px 10px' }}>
                      <button className="kz-btn kz-btn-ghost kz-btn-sm" style={{ width: '100%', justifyContent: 'center', borderStyle: 'dashed', borderColor: 'var(--border)' }}
                        onClick={() => { setAddDay(di); setAddOpen(true); }}>
                        <Icons.Plus size={13}/>Add
                      </button>
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      </div>

      {/* Add Entry Dialog */}
      <AddEntryDialog open={addOpen} onClose={() => setAddOpen(false)}
        dayIdx={addDay} onAdd={handleAddEntry}/>
      
      {/* Edit Entry Dialog */}
      <EditEntryDialog entry={editEntry} onClose={() => setEditEntry(null)}
        onDelete={(di, ei) => { handleDeleteEntry(di, ei); setEditEntry(null); }}
        editable={status === 'draft'}/>
    </div>
  );
};

// ── Add Entry Dialog ──
const AddEntryDialog = ({ open, onClose, dayIdx, onAdd }) => {
  const [cat, setCat] = React.useState('gruppe');
  const [start, setStart] = React.useState('08:00');
  const [end, setEnd] = React.useState('12:00');

  const handleSave = () => {
    onAdd(dayIdx, { cat, start, end });
    setCat('gruppe'); setStart('08:00'); setEnd('12:00');
  };

  return (
    <Dialog open={open} onClose={onClose} title={`Add Entry · ${DAYS_FULL[dayIdx] || ''}`}>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
        <div>
          <label className="kz-label">Category</label>
          <select className="kz-select" value={cat} onChange={e => setCat(e.target.value)}>
            {CATEGORIES.map(c => <option key={c.id} value={c.id}>{c.label}</option>)}
          </select>
        </div>
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <div>
            <label className="kz-label">Start</label>
            <input className="kz-input" type="time" value={start} onChange={e => setStart(e.target.value)}/>
          </div>
          <div>
            <label className="kz-label">End</label>
            <input className="kz-input" type="time" value={end} onChange={e => setEnd(e.target.value)}/>
          </div>
        </div>
        {start && end && timeToMin(end) > timeToMin(start) && (
          <div style={{ fontSize: 13, color: 'var(--text-tertiary)' }} className="tab-num">
            Duration: {duration(start, end)}h
          </div>
        )}
        <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end', paddingTop: 4 }}>
          <button className="kz-btn" onClick={onClose}>Cancel</button>
          <button className="kz-btn kz-btn-primary" onClick={handleSave}>Add Entry</button>
        </div>
      </div>
    </Dialog>
  );
};

// ── Edit Entry Dialog ──
const EditEntryDialog = ({ entry, onClose, onDelete, editable }) => {
  if (!entry) return null;
  const cat = CATEGORIES.find(c => c.id === entry.cat);
  return (
    <Dialog open={!!entry} onClose={onClose} title="Entry Details">
      <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <CatDot catId={entry.cat} size={10}/>
          <span style={{ fontWeight: 500 }}>{cat?.label}</span>
        </div>
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <div><span className="kz-label">Start</span><div className="tab-num" style={{ fontSize: 16, fontWeight: 500 }}>{entry.start}</div></div>
          <div><span className="kz-label">End</span><div className="tab-num" style={{ fontSize: 16, fontWeight: 500 }}>{entry.end}</div></div>
        </div>
        <div><span className="kz-label">Duration</span><div className="tab-num" style={{ fontSize: 16, fontWeight: 500 }}>{duration(entry.start, entry.end)}h</div></div>
        {editable && (
          <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end', paddingTop: 8 }}>
            <button className="kz-btn kz-btn-danger" onClick={() => onDelete(entry.di, entry.ei)}>
              <Icons.Trash size={14}/>Delete
            </button>
            <button className="kz-btn" onClick={onClose}>Close</button>
          </div>
        )}
      </div>
    </Dialog>
  );
};


// ── Mobile Time Entry ──
const TimeEntryMobile = () => {
  const week = genWeekEntries();
  const [selDay, setSelDay] = React.useState(0);
  const [status] = React.useState('draft');
  const dayEntries = week[selDay];
  const total = dayTotal(dayEntries);
  const weekTotal = week.reduce((s, d) => s + dayTotal(d), 0);
  const dates = ['16', '17', '18', '19', '20'];

  return (
    <div className="kz" style={{ height: '100%', background: 'var(--bg-canvas)', display: 'flex', flexDirection: 'column' }}>
      {/* Mobile header */}
      <div style={{ background: 'var(--nav-bg)', color: '#fff', padding: '12px 16px 0' }}>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 12 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <div style={{ width: 26, height: 26, borderRadius: 6, background: 'var(--accent)', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
              <Icons.Clock size={13} style={{ stroke: '#fff' }}/>
            </div>
            <span style={{ fontWeight: 600, fontSize: 15 }}>KitaZeit</span>
          </div>
          <Avatar initials={ME.avatar} size={28} bg="var(--nav-bg-active)" color="#fff"/>
        </div>

        {/* Week selector */}
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 10 }}>
          <Icons.ChevLeft size={18} style={{ opacity: 0.6, cursor: 'pointer' }}/>
          <span style={{ fontSize: 13, fontWeight: 500 }} className="tab-num">Dec 16 – 20, 2024</span>
          <Icons.ChevRight size={18} style={{ opacity: 0.6, cursor: 'pointer' }}/>
        </div>

        {/* Day pills */}
        <div style={{ display: 'flex', gap: 4, paddingBottom: 12 }}>
          {DAYS.map((d, i) => (
            <button key={d} onClick={() => setSelDay(i)} style={{
              flex: 1, padding: '6px 0', borderRadius: 'var(--radius-md)',
              border: 'none', cursor: 'pointer', textAlign: 'center',
              background: i === selDay ? 'var(--accent)' : 'transparent',
              color: i === selDay ? '#fff' : 'var(--nav-text)',
              fontFamily: 'inherit', fontSize: 11.5, fontWeight: 500,
              transition: 'background .12s',
            }}>
              <div>{d}</div>
              <div className="tab-num" style={{ fontSize: 15, fontWeight: 600, marginTop: 1 }}>{dates[i]}</div>
            </button>
          ))}
        </div>
      </div>

      {/* Summary row */}
      <div style={{ padding: '12px 16px', display: 'flex', gap: 10, borderBottom: '1px solid var(--border)' }}>
        <div style={{ flex: 1 }}>
          <div style={{ fontSize: 10.5, color: 'var(--text-tertiary)', textTransform: 'uppercase', fontWeight: 500, letterSpacing: '0.04em' }}>Today</div>
          <div style={{ fontSize: 18, fontWeight: 600, color: 'var(--accent)' }} className="tab-num">{total.toFixed(1)}h</div>
        </div>
        <div style={{ flex: 1 }}>
          <div style={{ fontSize: 10.5, color: 'var(--text-tertiary)', textTransform: 'uppercase', fontWeight: 500, letterSpacing: '0.04em' }}>Week</div>
          <div style={{ fontSize: 18, fontWeight: 600 }} className="tab-num">{weekTotal.toFixed(1)}h</div>
        </div>
        <div>
          <StatusChip status={status}/>
        </div>
      </div>

      {/* Entries list */}
      <div style={{ flex: 1, overflow: 'auto', padding: '12px 16px' }} className="kz-scroll">
        <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
          {dayEntries.map((entry, i) => {
            const cat = CATEGORIES.find(c => c.id === entry.cat);
            return (
              <div key={i} className="kz-card" style={{ padding: '12px 14px', display: 'flex', alignItems: 'center', gap: 12 }}>
                <div style={{ width: 4, height: 36, borderRadius: 2, background: cat?.color, flexShrink: 0 }}/>
                <div style={{ flex: 1 }}>
                  <div style={{ fontSize: 13, fontWeight: 500, marginBottom: 2 }}>{cat?.label}</div>
                  <div style={{ fontSize: 12, color: 'var(--text-tertiary)' }} className="tab-num">{entry.start} – {entry.end}</div>
                </div>
                <div style={{ fontSize: 15, fontWeight: 600, color: 'var(--text-secondary)' }} className="tab-num">{duration(entry.start, entry.end)}h</div>
              </div>
            );
          })}
        </div>

        {/* Add button */}
        <button className="kz-btn kz-btn-primary" style={{ width: '100%', marginTop: 12, justifyContent: 'center', height: 42 }}>
          <Icons.Plus size={16}/>Add Entry
        </button>
      </div>

      {/* Bottom nav */}
      <MobileNav active="time"/>
    </div>
  );
};

const MobileNav = ({ active = 'time' }) => {
  const items = [
    { id: 'time', icon: 'Clock', label: 'Time' },
    { id: 'absences', icon: 'Plane', label: 'Absences' },
    { id: 'calendar', icon: 'Calendar', label: 'Calendar' },
    { id: 'account', icon: 'User', label: 'Account' },
  ];
  return (
    <div style={{
      borderTop: '1px solid var(--border)', background: 'var(--bg-surface)',
      display: 'flex', padding: '6px 0 2px',
    }}>
      {items.map(it => {
        const Ic = Icons[it.icon];
        const isActive = active === it.id;
        return (
          <div key={it.id} style={{
            flex: 1, display: 'flex', flexDirection: 'column', alignItems: 'center',
            gap: 2, padding: '4px 0', cursor: 'pointer',
            color: isActive ? 'var(--accent)' : 'var(--text-tertiary)',
          }}>
            <Ic size={20} style={{ strokeWidth: isActive ? 2 : 1.6 }}/>
            <span style={{ fontSize: 10, fontWeight: isActive ? 600 : 400 }}>{it.label}</span>
          </div>
        );
      })}
    </div>
  );
};

Object.assign(window, { TimeEntryDesktop, TimeEntryMobile, MobileNav, AddEntryDialog, EditEntryDialog });
