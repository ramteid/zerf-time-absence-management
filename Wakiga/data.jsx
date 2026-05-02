// KitaZeit demo data
const KZ_CATEGORIES = {
  gruppe: { name: 'Group time', short: 'Group', color: 'var(--cat-gruppe)' },
  vorbereitung: { name: 'Preparation', short: 'Prep', color: 'var(--cat-vorbereitung)' },
  elterngespraech: { name: 'Parent meeting', short: 'Parent mtg', color: 'var(--cat-elterngespraech)' },
  team: { name: 'Team meeting', short: 'Team', color: 'var(--cat-team)' },
  fortbildung: { name: 'Training', short: 'Training', color: 'var(--cat-fortbildung)' },
  pause: { name: 'Break', short: 'Break', color: 'var(--cat-pause)' },
};

// Week 18 — Mon 27 Apr – Sun 3 May 2026 (today: Wed 29 Apr)
// status: draft | submitted | approved | rejected
const KZ_WEEK = {
  number: 18,
  start: 'Mon 27 Apr',
  end: 'Sun 3 May 2026',
  targetMin: 32 * 60, // 32h/wk part-time = 80%
  days: [
    { date: 'Mon 27', dow: 'Monday', dateLong: '27 April', isToday: false, entries: [
      { id: 'e1', start: '07:30', end: '09:00', cat: 'vorbereitung', dur: 90, status: 'draft', comment: 'Morning prep, room setup' },
      { id: 'e2', start: '09:00', end: '12:30', cat: 'gruppe', dur: 210, status: 'draft', comment: 'Bärengruppe' },
      { id: 'e3', start: '12:30', end: '13:00', cat: 'pause', dur: 30, status: 'draft' },
      { id: 'e4', start: '13:00', end: '15:00', cat: 'gruppe', dur: 120, status: 'draft', comment: 'Bärengruppe' },
    ]},
    { date: 'Tue 28', dow: 'Tuesday', dateLong: '28 April', isToday: false, entries: [
      { id: 'e5', start: '07:30', end: '12:30', cat: 'gruppe', dur: 300, status: 'draft', comment: 'Bärengruppe' },
      { id: 'e6', start: '12:30', end: '13:00', cat: 'pause', dur: 30, status: 'draft' },
      { id: 'e7', start: '13:00', end: '15:30', cat: 'gruppe', dur: 150, status: 'draft' },
      { id: 'e8', start: '15:30', end: '16:30', cat: 'team', dur: 60, status: 'draft', comment: 'Weekly team round' },
    ]},
    { date: 'Wed 29', dow: 'Wednesday', dateLong: '29 April', isToday: true, entries: [
      { id: 'e9', start: '07:30', end: '09:00', cat: 'vorbereitung', dur: 90, status: 'draft' },
      { id: 'e10', start: '09:00', end: '12:30', cat: 'gruppe', dur: 210, status: 'draft' },
      { id: 'e11', start: '14:00', end: '15:00', cat: 'elterngespraech', dur: 60, status: 'draft', comment: 'Family Becker' },
    ]},
    { date: 'Thu 30', dow: 'Thursday', dateLong: '30 April', isToday: false, entries: [] },
    { date: 'Fri 01', dow: 'Friday', dateLong: '1 May', isToday: false, holiday: 'Labour Day', entries: [] },
    { date: 'Sat 02', dow: 'Saturday', dateLong: '2 May', isToday: false, weekend: true, entries: [] },
    { date: 'Sun 03', dow: 'Sunday', dateLong: '3 May', isToday: false, weekend: true, entries: [] },
  ],
};

const KZ_USER = {
  name: 'Lena Hofmann',
  role: 'Educator · Bärengruppe',
  initials: 'LH',
  weeklyTargetH: 32,
  vacationLeft: 11,
  vacationTotal: 28,
  overtime: 4.5,
  sickDays: 2,
};

// Approval queue for lead view
const KZ_APPROVALS = {
  timesheets: [
    { id: 't1', name: 'Sophie Bauer', role: 'Educator · Mäusegruppe', initials: 'SB', week: 17, hours: '32:15', target: '32:00', diff: '+0:15', entries: 12, submitted: '2 days ago' },
    { id: 't2', name: 'Marek Vogel', role: 'Educator · Bärengruppe', initials: 'MV', week: 17, hours: '40:00', target: '40:00', diff: '0:00', entries: 18, submitted: '2 days ago' },
    { id: 't3', name: 'Anna Richter', role: 'Trainee', initials: 'AR', week: 17, hours: '38:30', target: '40:00', diff: '−1:30', entries: 14, submitted: 'yesterday' },
    { id: 't4', name: 'Julia Wend', role: 'Educator · Spatzengruppe', initials: 'JW', week: 17, hours: '30:00', target: '30:00', diff: '0:00', entries: 11, submitted: 'yesterday' },
  ],
  absences: [
    { id: 'a1', name: 'Marek Vogel', initials: 'MV', type: 'Vacation', from: '11 May', to: '15 May', days: 5, balance: '14 days left', submitted: '3 days ago' },
    { id: 'a2', name: 'Anna Richter', initials: 'AR', type: 'Sick leave', from: '28 Apr', to: '29 Apr', days: 2, note: 'Doctor\u2019s note attached', submitted: 'today' },
    { id: 'a3', name: 'Sophie Bauer', initials: 'SB', type: 'Vacation', from: '20 Jul', to: '7 Aug', days: 14, balance: '6 days left after', submitted: 'yesterday' },
  ],
  changeRequests: [
    { id: 'c1', name: 'Julia Wend', initials: 'JW', date: 'Wed 22 Apr', orig: '13:00 – 15:00 Group', proposed: '13:00 – 16:00 Group', reason: 'Stayed late for room handover, forgot to update.', submitted: 'today' },
  ],
};

const KZ_TEAM_CALENDAR = [
  { name: 'Lena Hofmann', initials: 'LH', items: [] },
  { name: 'Sophie Bauer', initials: 'SB', items: [{ start: 4, len: 5, type: 'vacation', label: 'Vacation' }] },
  { name: 'Marek Vogel', initials: 'MV', items: [{ start: 11, len: 5, type: 'vacation', label: 'Vacation' }] },
  { name: 'Anna Richter', initials: 'AR', items: [{ start: 1, len: 2, type: 'sick', label: 'Sick' }] },
  { name: 'Julia Wend', initials: 'JW', items: [{ start: 18, len: 3, type: 'training', label: 'Training' }] },
  { name: 'Tobias Klein', initials: 'TK', items: [{ start: 25, len: 4, type: 'vacation', label: 'Vacation' }] },
];

window.KZ_CATEGORIES = KZ_CATEGORIES;
window.KZ_WEEK = KZ_WEEK;
window.KZ_USER = KZ_USER;
window.KZ_APPROVALS = KZ_APPROVALS;
window.KZ_TEAM_CALENDAR = KZ_TEAM_CALENDAR;

// helpers
window.kzFmtHM = (mins) => {
  const h = Math.floor(Math.abs(mins) / 60);
  const m = Math.abs(mins) % 60;
  return `${mins < 0 ? '−' : ''}${h}:${String(m).padStart(2, '0')}`;
};
