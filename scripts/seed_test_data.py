#!/usr/bin/env python3
"""
Zerf test-data seeder.

Generates a complete, internally consistent set of test data into an *empty*
Zerf production-style database.  Designed for safe execution against a
freshly deployed instance — refuses to run if business data already exists,
unless explicitly invoked with --wipe.

The generated data set covers one user per role (admin, team_lead, employee,
assistant) plus realistic personal backstories that explain the time entries
and absences:

  Arnold Admin      (admin)      — purely administrative; tracks_time=FALSE,
                                   no time entries, no absences.
  Tabea Teamlead    (team_lead)  — works as senior educator, runs the team,
                                   sole approver for Tabea is Arnold (admin).
  Eva Erzieherin    (employee)   — main educator; default approver Tabea,
                                   occasionally approved by Arnold.
  Alina Aushilfe    (assistant)  — on-call springer; max 40h/month, mostly
                                   less, called in when Eva or Tabea is out.

Each non-admin user gets entries for every contract workday since their hire
date, mixed with absences (vacation, sick, training, special leave, flextime
reduction, cancellations).  Reopen-requests are generated for all workflow
paths (pending, approved, rejected).

The script targets migration version 14 (see backend/migrations/) and respects
all CHECK constraints (role, status, kind, time format, workdays_per_week).
At-rest encryption (pg_tde) is fully transparent on the SQL layer; the script
only loads ZERF_DB_ENCRYPTION_KEY to verify it is present.

SAFETY GUARD
------------
The script refuses to run as soon as it finds any row in the `users`
table — that is, as soon as the Zerf admin setup flow (/auth/setup) has
been completed.  There is no --force or --wipe flag to bypass this guard.
Re-seeding an already-bootstrapped database requires dropping the postgres
data volume manually first.

USAGE
-----
Run locally on the host that owns the postgres container (e.g. the prod
server, ssh'd in directly).  The script reads ZERF_POSTGRES_USER / _PASSWORD /
_DB from `.env` automatically (or accepts PG* overrides on the environment).

On the prod stack postgres is on the internal-only `docker_private` network
and is not published to the host port.  When PGHOST is empty or set to a
container name, the script automatically resolves the container's docker IP
via `docker inspect` and connects through that.  No port-forwarding or
sidecar container is required.

    # On the prod host, in the repo checkout (zerf2/):
    sudo apt install -y python3-psycopg2 python3-dotenv python3-argon2
    python3 scripts/seed_test_data.py --yes

    # Dry-run that connects and runs everything in one tx, then rolls back:
    python3 scripts/seed_test_data.py --yes --dry-run

If `python3-argon2` is missing on prod, install it via apt or fall back to:
    pip install --break-system-packages psycopg2-binary argon2-cffi python-dotenv

REQUIRED PYTHON PACKAGES
------------------------
    psycopg2 (or psycopg2-binary), argon2-cffi, python-dotenv

EXIT CODES
----------
    0  success
    1  refusal (setup already complete, missing env vars, etc.)
    2  fatal error during seeding (rolled back)
"""

from __future__ import annotations

import argparse
import os
import random
import secrets
import subprocess
import sys
from dataclasses import dataclass, field
from datetime import date, datetime, time, timedelta, timezone
from pathlib import Path
from typing import Callable, Iterable, Iterator

try:
    import psycopg2
    import psycopg2.extras
    from argon2 import PasswordHasher
    from argon2.low_level import Type as Argon2Type
    from dotenv import dotenv_values
except ImportError as exc:  # pragma: no cover — environment guard, no business logic
    sys.stderr.write(
        f"\nERROR: missing required package ({exc.name}).\n"
        "Install:  pip install psycopg2-binary argon2-cffi python-dotenv\n\n"
    )
    sys.exit(1)


# ---------------------------------------------------------------------------
# Reference values aligned with backend/services/auth.rs and backend/migrations
# ---------------------------------------------------------------------------

# Pinned reference date.  Matches the TEST_REFERENCE_DATE pattern from
# backend/services/settings.rs and the production test database snapshot the
# script was validated against (today=2026-05-26, a Tuesday).
TODAY = date(2026, 5, 26)

SEED = 20260526  # deterministic randomness — every run produces the same data
RNG = random.Random(SEED)

# Argon2id parameters MUST match backend/services/auth.rs::argon2_instance.
# Mismatched params would still verify, but PHC strings would drift between
# the seeder and any in-app password change, complicating debugging.
ARGON2 = PasswordHasher(
    time_cost=2,
    memory_cost=19456,
    parallelism=1,
    hash_len=32,
    salt_len=16,
    type=Argon2Type.ID,
)

# Category IDs reflect the order the application bootstraps them in.
# The script creates the exact same set deterministically so generated
# time entries can reference them by stable id.  Names and description
# are in German to match the seeded `ui_language=de` setting (the
# application's auto-seed uses English names; we override here for a
# coherent German demo experience).
CATEGORIES = [
    # (id, name, color, sort_order, counts_as_work, description)
    (1, "Arbeit am Kind",  "#4CAF50", 1, True,  None),
    (2, "Vorbereitung",    "#22c55e", 2, True,  None),
    (3, "Leitungsaufgaben","#84cc16", 3, True,  None),
    (4, "Teambesprechung", "#7c3aed", 4, True,  None),
    (5, "Fortbildung",     "#795548", 5, True,  None),
    (6, "Sonstiges",       "#607D8B", 6, True,  None),
    (7, "Gleitzeitabbau",  "#6D4C41", 7, False,
     "Blockt Zeit ohne Arbeitsstunden gutzuschreiben."),
]

CAT_WORK = 1
CAT_PREP = 2
CAT_LEAD = 3
CAT_MEETING = 4
CAT_TRAINING = 5
CAT_OTHER = 6
CAT_FLEX = 7

# DE-BW public holidays (matches the prod holidays table snapshot).
HOLIDAYS_BW = [
    # 2026
    (date(2026, 1, 1),  "New Year's Day",     "Neujahr"),
    (date(2026, 1, 6),  "Epiphany",           "Heilige Drei Könige"),
    (date(2026, 4, 3),  "Good Friday",        "Karfreitag"),
    (date(2026, 4, 6),  "Easter Monday",      "Ostermontag"),
    (date(2026, 5, 1),  "Labour Day",         "Tag der Arbeit"),
    (date(2026, 5, 14), "Ascension Day",      "Christi Himmelfahrt"),
    (date(2026, 5, 25), "Whit Monday",        "Pfingstmontag"),
    (date(2026, 6, 4),  "Corpus Christi",     "Fronleichnam"),
    (date(2026, 10, 3), "German Unity Day",   "Tag der Deutschen Einheit"),
    (date(2026, 11, 1), "All Saints' Day",    "Allerheiligen"),
    (date(2026, 12, 25), "Christmas Day",     "Erster Weihnachtstag"),
    (date(2026, 12, 26), "St. Stephen's Day", "Zweiter Weihnachtstag"),
    # 2027
    (date(2027, 1, 1),  "New Year's Day",     "Neujahr"),
    (date(2027, 1, 6),  "Epiphany",           "Heilige Drei Könige"),
    (date(2027, 3, 26), "Good Friday",        "Karfreitag"),
    (date(2027, 3, 29), "Easter Monday",      "Ostermontag"),
    (date(2027, 5, 1),  "Labour Day",         "Tag der Arbeit"),
    (date(2027, 5, 6),  "Ascension Day",      "Christi Himmelfahrt"),
    (date(2027, 5, 17), "Whit Monday",        "Pfingstmontag"),
    (date(2027, 5, 27), "Corpus Christi",     "Fronleichnam"),
    (date(2027, 10, 3), "German Unity Day",   "Tag der Deutschen Einheit"),
    (date(2027, 11, 1), "All Saints' Day",    "Allerheiligen"),
    (date(2027, 12, 25), "Christmas Day",     "Erster Weihnachtstag"),
    (date(2027, 12, 26), "St. Stephen's Day", "Zweiter Weihnachtstag"),
]
HOLIDAY_DATES = frozenset(h[0] for h in HOLIDAYS_BW)

# app_settings — chosen to mirror the production snapshot so the UI behaves
# identically.  Keys not listed here keep the application defaults baked into
# backend/services/settings.rs.
APP_SETTINGS = {
    "ui_language": "de",
    "time_format": "24h",
    "timezone": "Europe/Berlin",
    "country": "DE",
    "region": "DE-BW",
    "default_weekly_hours": "39",
    "default_annual_leave_days": "30",
    "carryover_expiry_date": "03-31",
    "submission_deadline_day": "5",
    "organization_name": "Waldkindergarten Gundelfingen",
    "submission_reminders_enabled": "true",
    "approval_reminders_enabled": "true",
    "smtp_enabled": "false",
}


# ---------------------------------------------------------------------------
# Personas
# ---------------------------------------------------------------------------

@dataclass
class Persona:
    key: str
    email: str
    first_name: str
    last_name: str
    role: str
    weekly_hours: float
    workdays_per_week: int
    start_date: date
    overtime_start_balance_min: int
    tracks_time: bool
    allow_reopen_without_approval: bool
    must_change_password: bool
    password: str
    annual_leave_days: dict[int, int] = field(default_factory=dict)
    # Filled in at runtime once the user row is inserted:
    user_id: int = 0


PERSONAS: list[Persona] = [
    Persona(
        key="admin",
        email="arnold.admin@waldkindergarten-gundelfingen.de",
        first_name="Arnold",
        last_name="Admin",
        role="admin",
        # Admin does not track time — tracks_time=FALSE.  The CHECK constraint
        # `users_admin_only_no_tracks_time` only permits this for admin roles.
        weekly_hours=0.0,
        workdays_per_week=5,
        start_date=date(2025, 6, 1),
        overtime_start_balance_min=0,
        tracks_time=False,
        allow_reopen_without_approval=False,
        must_change_password=False,
        password="Admin!Pass-2026",
        annual_leave_days={2026: 30, 2027: 30},
    ),
    Persona(
        key="team_lead",
        email="tabea.teamlead@waldkindergarten-gundelfingen.de",
        first_name="Tabea",
        last_name="Teamlead",
        role="team_lead",
        weekly_hours=39.0,
        workdays_per_week=5,
        start_date=date(2026, 1, 5),  # Mon, ISO week 2
        # 500 minutes ≈ 8h 20min carried over from prior employment, visible
        # in dashboards as "Stunden zu Beginn".
        overtime_start_balance_min=500,
        tracks_time=True,
        allow_reopen_without_approval=False,
        must_change_password=False,
        password="TeamLead!2026",
        annual_leave_days={2026: 30, 2027: 30},
    ),
    Persona(
        key="employee",
        email="eva.erzieherin@waldkindergarten-gundelfingen.de",
        first_name="Eva",
        last_name="Erzieherin",
        role="employee",
        weekly_hours=39.0,
        workdays_per_week=5,
        start_date=date(2026, 1, 5),
        overtime_start_balance_min=300,  # +5h from previous year carryover
        tracks_time=True,
        allow_reopen_without_approval=False,
        must_change_password=False,
        password="Erzieher!2026",
        annual_leave_days={2026: 30, 2027: 30},
    ),
    Persona(
        key="assistant",
        email="alina.aushilfe@waldkindergarten-gundelfingen.de",
        first_name="Alina",
        last_name="Aushilfe",
        role="assistant",
        # Assistants have weekly_hours=0 — no fixed schedule.  Their daily
        # target is 0 by definition; the submission-status logic exempts them
        # (see backend/services/reports.rs::submission_status_for_month).
        weekly_hours=0.0,
        # workdays_per_week=7 so any weekday/weekend can host an entry.
        workdays_per_week=7,
        start_date=date(2026, 3, 2),  # Mon
        overtime_start_balance_min=0,
        tracks_time=True,
        allow_reopen_without_approval=False,
        must_change_password=False,
        password="Aushilfe!2026",
        # Assistants typically have no annual-leave entitlement.  Recording
        # zero days explicitly keeps the dashboards consistent.
        annual_leave_days={2026: 0, 2027: 0},
    ),
]


# ---------------------------------------------------------------------------
# Story knobs — narrative parameters that shape the generated data.
# ---------------------------------------------------------------------------

# Absences anchor the schedule: time entries are NEVER created on absence days.
# Each tuple: (persona_key, kind, start, end, status, cancellation?)
#
# Status values match backend/migrations/001 & 003:
#   requested, approved, rejected, cancelled, cancellation_pending
# Kind values match backend/migrations/008:
#   vacation, sick, training, special_leave, unpaid,
#   general_absence, flextime_reduction
ABSENCE_SCRIPT: list[tuple[str, str, date, date, str, str | None]] = [
    # ── Tabea (team_lead) ────────────────────────────────────────────────
    # Winter break around her start date — already approved.
    ("team_lead", "vacation",  date(2026, 2, 16), date(2026, 2, 20), "approved",  "Skiurlaub mit Familie"),
    # Leadership training, 1 day.
    ("team_lead", "training",  date(2026, 3, 17), date(2026, 3, 17), "approved",  "Führungskräfte-Coaching (online)"),
    # Single sick day in May.
    ("team_lead", "sick",      date(2026, 5, 7),  date(2026, 5, 7),  "approved",  "Erkältung"),
    # Big summer holiday — future, already approved by Arnold.
    ("team_lead", "vacation",  date(2026, 8, 3),  date(2026, 8, 14), "approved",  "Sommerurlaub"),
    # Pending request waiting on Arnold.
    ("team_lead", "vacation",  date(2026, 10, 12), date(2026, 10, 16), "requested", "Herbsturlaub"),

    # ── Eva (employee) ───────────────────────────────────────────────────
    ("employee", "sick",       date(2026, 2, 23), date(2026, 2, 24), "approved",  "Magen-Darm"),
    ("employee", "training",   date(2026, 3, 10), date(2026, 3, 11), "approved",  "Fortbildung Naturpädagogik"),
    ("employee", "vacation",   date(2026, 4, 13), date(2026, 4, 17), "approved",  "Frühlingsferien"),
    ("employee", "sick",       date(2026, 5, 11), date(2026, 5, 12), "approved",  "Migräne"),
    # An old special_leave request that Eva later cancelled.
    ("employee", "special_leave", date(2026, 4, 24), date(2026, 4, 24), "cancelled", "Umzug — doch nicht nötig"),
    # An old training that was rejected.
    ("employee", "training",   date(2026, 4, 27), date(2026, 4, 28), "rejected",  "Externes Fachseminar"),
    # A flextime reduction day (compensates overtime) — counts as off but not paid leave.
    ("employee", "flextime_reduction", date(2026, 5, 22), date(2026, 5, 22), "approved", "Überstundenausgleich"),
    # Cancellation pending (user requested to cancel an approved absence).
    ("employee", "vacation",   date(2026, 6, 29), date(2026, 7, 3),   "cancellation_pending", "Doch lieber später"),
    # Big summer holiday — future, approved.
    ("employee", "vacation",   date(2026, 7, 20), date(2026, 7, 31),  "approved",  "Sommerurlaub am Bodensee"),
    # An unpaid leave example.
    ("employee", "unpaid",     date(2026, 9, 14), date(2026, 9, 15),  "approved",  "Privater Termin"),

    # ── Alina (assistant) ────────────────────────────────────────────────
    # Sick on a day she was actually scheduled — she was filling in for Eva's
    # spring break (2026-04-13..04-17), got sick on day 2 of that cover shift.
    ("assistant", "sick",      date(2026, 4, 14), date(2026, 4, 14),  "approved",  "Erkältung"),
    # Future general_absence — exam day.
    ("assistant", "general_absence", date(2026, 6, 22), date(2026, 6, 22), "requested", "Klausur an der Hochschule"),
]


# Each tuple: (persona_key, week_monday, status, reason, rejection_reason)
# Statuses follow migration 001 + 002: pending, approved, auto_approved, rejected.
# 'pending' requires reviewed_by IS NULL (CHECK reopen_requests_reviewed_by_pending).
REOPEN_SCRIPT: list[tuple[str, date, str, str, str | None]] = [
    # Eva: corrected a Thursday entry from week 14 — approved by Tabea.
    # Friday of this week is Good Friday (no entry), so the correction must
    # target Thursday 2026-04-02 which does have entries.
    ("employee", date(2026, 3, 30), "approved", "Falsche Endzeit am Donnerstag, bitte um Korrektur.", None),
    # Eva: tried to reopen week 16 — Tabea rejected (week already closed).
    ("employee", date(2026, 4, 20), "rejected", "Falsche Startzeit am Dienstag.", "Bitte nächste Woche direkt korrekt erfassen."),
    # Eva: more recent reopen, escalated directly to Arnold (admin) because
    # the correction was time-sensitive and Tabea hadn't reviewed yet.
    ("employee", date(2026, 5, 4),  "approved", "Pausenzeiten am Mittwoch korrigieren.", None),
    # Eva: currently pending — waiting for Tabea/Arnold to review (week just submitted).
    ("employee", date(2026, 5, 18), "pending",  "Eine Stunde am Donnerstag fehlt — Elterngespräch.", None),
    # Tabea: one reopen on her own week, Arnold approved.
    ("team_lead", date(2026, 3, 23), "approved", "Leadership-Block falsch zugeordnet.", None),
]


# ---------------------------------------------------------------------------
# Generation primitives
# ---------------------------------------------------------------------------

def week_monday(d: date) -> date:
    """ISO week Monday for the given date."""
    return d - timedelta(days=d.weekday())


def iter_dates(start: date, end_inclusive: date) -> Iterator[date]:
    cur = start
    while cur <= end_inclusive:
        yield cur
        cur += timedelta(days=1)


def is_contract_workday(persona: Persona, d: date) -> bool:
    """Mon=0 .. Sun=6 — contract workdays are the first N days of the ISO week."""
    return d.weekday() < persona.workdays_per_week


def fmt_time(t: time) -> str:
    return t.strftime("%H:%M")


def random_local_datetime(d: date, rng: random.Random, hour_window: tuple[int, int] = (17, 21)) -> datetime:
    """Generate a plausible audit timestamp (local time then naive UTC offset).

    We persist these as UTC; the small drift is acceptable for test data.
    """
    h = rng.randint(*hour_window)
    m = rng.randint(0, 59)
    return datetime.combine(d, time(h, m, rng.randint(0, 59)), tzinfo=timezone.utc)


# Pre-built day patterns for the educators.  Each pattern is a list of
# (start_time, end_time, category_id) covering ~daily target (7.8h).  By
# rotating through patterns we get visible variety without introducing
# logic errors (overlaps, ordering issues, daily total drift).
# Every pattern below sums to exactly the daily target of 7h48min (= 7.8 h),
# which matches weekly_hours=39 / workdays_per_week=5.  Verified by
# `_pattern_hours()` self-check at module load.
DAY_PATTERNS_EVA: list[list[tuple[str, str, int]]] = [
    [("07:30", "12:00", CAT_WORK),  ("12:30", "15:48", CAT_WORK)],
    [("08:00", "12:30", CAT_WORK),  ("13:00", "16:18", CAT_WORK)],
    [("07:45", "12:15", CAT_WORK),  ("12:45", "16:03", CAT_PREP)],
    [("08:00", "12:00", CAT_WORK),  ("12:30", "14:00", CAT_WORK),
     ("14:00", "16:18", CAT_PREP)],
    [("07:30", "11:30", CAT_WORK),  ("12:00", "14:00", CAT_WORK),
     ("14:00", "15:48", CAT_PREP)],
    [("08:00", "10:00", CAT_WORK),  ("10:00", "10:30", CAT_MEETING),
     ("10:30", "12:30", CAT_WORK),  ("13:00", "16:18", CAT_WORK)],
    [("07:30", "12:00", CAT_WORK),  ("12:30", "15:00", CAT_WORK),
     ("15:00", "15:48", CAT_PREP)],
]

DAY_PATTERNS_TABEA: list[list[tuple[str, str, int]]] = [
    # Monday — sprint planning + leadership block.
    [("08:00", "09:00", CAT_LEAD),  ("09:00", "12:30", CAT_WORK),
     ("13:00", "16:18", CAT_WORK)],
    # Tuesday — team meeting morning.
    [("08:00", "09:30", CAT_MEETING), ("09:30", "12:30", CAT_WORK),
     ("13:00", "16:18", CAT_WORK)],
    # Wednesday — kinder-focused.
    [("07:45", "12:15", CAT_WORK),  ("12:45", "16:03", CAT_WORK)],
    # Thursday — afternoon leadership + admin.
    [("08:00", "12:00", CAT_WORK),  ("12:30", "14:30", CAT_LEAD),
     ("14:30", "16:18", CAT_OTHER)],
    # Friday — short prep tail.
    [("08:00", "11:30", CAT_WORK),  ("12:00", "14:30", CAT_WORK),
     ("14:30", "16:18", CAT_PREP)],
]


def _pattern_minutes(pattern: list[tuple[str, str, int]]) -> int:
    total = 0
    for start_s, end_s, _cat in pattern:
        h1, m1 = (int(x) for x in start_s.split(":"))
        h2, m2 = (int(x) for x in end_s.split(":"))
        total += (h2 * 60 + m2) - (h1 * 60 + m1)
    return total


# Module-load self-check: every pattern must hit the daily target precisely.
# Catches accidental drift when a pattern is edited later.
DAILY_TARGET_MIN = 7 * 60 + 48  # 7h48 == 7.8h
for _i, _p in enumerate(DAY_PATTERNS_EVA + DAY_PATTERNS_TABEA):
    _got = _pattern_minutes(_p)
    if _got != DAILY_TARGET_MIN:
        raise AssertionError(
            f"day pattern #{_i} totals {_got} min ({_got/60:.2f}h), "
            f"expected {DAILY_TARGET_MIN} min ({DAILY_TARGET_MIN/60:.2f}h): {_p}"
        )


def pick_day_pattern(persona: Persona, d: date, rng: random.Random) -> list[tuple[str, str, int]]:
    """Pick a daily entry pattern based on persona and weekday."""
    if persona.key == "team_lead":
        # Tuesday and Friday have fixed patterns; others rotate.
        weekday = d.weekday()  # Mon=0
        if weekday == 1:  # Tuesday — team meeting
            return DAY_PATTERNS_TABEA[1]
        if weekday == 4:  # Friday — short prep tail
            return DAY_PATTERNS_TABEA[4]
        return rng.choice(DAY_PATTERNS_TABEA[:1] + DAY_PATTERNS_TABEA[2:4])
    if persona.key == "employee":
        return rng.choice(DAY_PATTERNS_EVA)
    raise AssertionError(f"no day pattern for persona {persona.key}")


# ---------------------------------------------------------------------------
# Database operations
# ---------------------------------------------------------------------------

# Support tables we clear before re-inserting our canonical data.
# The application auto-seeds categories on first startup (see
# backend/repository/categories.rs::seed_defaults_if_empty) and the
# /auth/setup flow writes a few app_settings rows; both can collide with
# our explicit IDs/values, so we TRUNCATE them first.  _sqlx_migrations
# and system_metadata are PRESERVED — they describe the schema version,
# not user data.
SUPPORT_TABLES_TO_CLEAR_IN_ORDER = [
    "audit_log",
    "notifications",
    "password_reset_tokens",
    "sessions",
    "login_attempts",
    "holidays",
    "categories",
    "app_settings",
]


def env_value(env: dict[str, str | None], *keys: str, default: str | None = None) -> str | None:
    for k in keys:
        v = env.get(k)
        if v is not None and v != "":
            return v
    return default


def load_environment(env_file: Path) -> dict[str, str | None]:
    """Merge process env on top of .env file values (.env wins only if unset)."""
    file_values = dotenv_values(env_file) if env_file.exists() else {}
    merged: dict[str, str | None] = dict(file_values)
    for k, v in os.environ.items():
        merged[k] = v
    return merged


def resolve_docker_container_ip(name: str) -> str | None:
    """Look up a docker container's IPv4 address.  Returns None if docker is
    unavailable, the container doesn't exist, or it isn't attached to a
    network."""
    try:
        proc = subprocess.run(
            ["docker", "inspect", "-f",
             "{{range $net, $cfg := .NetworkSettings.Networks}}"
             "{{$cfg.IPAddress}} {{end}}",
             name],
            check=True, capture_output=True, text=True, timeout=5,
        )
    except (FileNotFoundError, subprocess.CalledProcessError, subprocess.TimeoutExpired):
        return None
    # First non-empty IP wins.  pg_tde sits on a single network in the prod
    # compose so the loop terminates immediately.
    for token in proc.stdout.strip().split():
        if token:
            return token
    return None


def connect(env: dict[str, str | None]) -> psycopg2.extensions.connection:
    raw_host = env_value(env, "PGHOST", "ZERF_POSTGRES_HOST", default="zerf-postgres")
    port = env_value(env, "PGPORT", "ZERF_POSTGRES_PORT", default="5432")
    user = env_value(env, "PGUSER", "ZERF_POSTGRES_USER")
    password = env_value(env, "PGPASSWORD", "ZERF_POSTGRES_PASSWORD")
    dbname = env_value(env, "PGDATABASE", "ZERF_POSTGRES_DB", default="zerf")
    if not user or not password:
        sys.stderr.write(
            "ERROR: PGUSER/PGPASSWORD (or ZERF_POSTGRES_USER/_PASSWORD) "
            "must be set in .env or the environment.\n"
        )
        sys.exit(1)

    # If PGHOST looks like a docker container name (no dots, not localhost),
    # try to resolve it via `docker inspect`.  This lets the script run
    # directly on the prod host and reach the postgres container even though
    # the docker_private network is internal-only and has no published port.
    host = raw_host
    looks_like_container = "." not in raw_host and raw_host not in ("localhost", "127.0.0.1")
    if looks_like_container:
        resolved = resolve_docker_container_ip(raw_host)
        if resolved:
            sys.stderr.write(f"Resolved container '{raw_host}' to {resolved} via docker inspect.\n")
            host = resolved
        else:
            sys.stderr.write(
                f"WARNING: could not resolve docker container '{raw_host}' — falling back to "
                "DNS.  Connection will fail unless the name is on the host's resolver.\n"
            )

    sys.stderr.write(f"Connecting to {user}@{host}:{port}/{dbname} ...\n")
    conn = psycopg2.connect(
        host=host, port=int(port), user=user, password=password, dbname=dbname,
        # Force a clean isolation boundary — all seed work runs inside one tx.
        application_name="zerf-seed-test-data",
        # The seeder data contains German diacritics (e.g. "Heilige Drei Könige").
        # If the postgres server's client_encoding inherits the LC_ALL=C default
        # of the prod image, psycopg2 silently encodes parameters as ASCII and
        # blows up on the first ü/ö/ß.  Pinning UTF-8 explicitly makes the
        # script portable across hosts.
        client_encoding="UTF8",
    )
    # Belt-and-suspenders: also configure the python-side encoding so any later
    # cursor inherits UTF-8 regardless of the server default.
    conn.set_client_encoding("UTF8")
    return conn


def fetch_one_int(cur, sql: str, params: tuple = ()) -> int:
    cur.execute(sql, params)
    row = cur.fetchone()
    return int(row[0]) if row else 0


def assert_setup_not_complete(cur) -> None:
    """Hard safety guard.

    The Zerf admin setup flow (/auth/setup) creates the first user row only
    after an operator has gone through the interactive bootstrap UI.  Any
    non-zero count in the `users` table therefore means a real human has
    already taken ownership of the deployment, and overwriting that data
    would be catastrophic (lost accounts, lost time entries, lost audit log).

    We refuse unconditionally — there is no --force flag to bypass this —
    so the only way to re-seed an existing deployment is to drop and
    recreate the postgres data volume manually.  This makes accidental
    seed-over-production impossible.
    """
    user_count = fetch_one_int(cur, "SELECT COUNT(*) FROM users")
    if user_count == 0:
        return
    cur.execute(
        "SELECT id, email, role, created_at FROM users ORDER BY id LIMIT 5"
    )
    sample = cur.fetchall()
    sys.stderr.write(
        "\nERROR: admin setup has already been completed on this database "
        f"({user_count} user row(s) present).\n"
        "       Refusing to overwrite real data.  If you are absolutely\n"
        "       certain you want to wipe this deployment, drop the postgres\n"
        "       data volume and redeploy before running the seeder again.\n\n"
        "       Existing users (up to 5):\n"
    )
    for row in sample:
        sys.stderr.write(f"         id={row[0]:4d}  role={row[2]:<10s}  email={row[1]}  created={row[3]}\n")
    sys.stderr.write("\n")
    sys.exit(1)


def clear_support_tables(cur) -> None:
    """Reset support tables (categories, holidays, app_settings, sessions,
    etc.) so the seeder can take ownership of stable IDs and known values.

    Safe to call only after `assert_setup_not_complete` has verified that
    no user rows exist — the `_sqlx_migrations` and `system_metadata`
    tables are intentionally PRESERVED.
    """
    for table in SUPPORT_TABLES_TO_CLEAR_IN_ORDER:
        cur.execute(f"TRUNCATE TABLE {table} RESTART IDENTITY CASCADE")
    sys.stderr.write(f"Cleared {len(SUPPORT_TABLES_TO_CLEAR_IN_ORDER)} support tables.\n")


def insert_categories(cur) -> None:
    for cat_id, name, color, sort_order, counts_as_work, description in CATEGORIES:
        cur.execute(
            """
            INSERT INTO categories(id, name, description, color, sort_order, active, counts_as_work)
            VALUES (%s, %s, %s, %s, %s, TRUE, %s)
            """,
            (cat_id, name, description, color, sort_order, counts_as_work),
        )
    # Bump the sequence past our explicit ids so future inserts don't collide.
    cur.execute("SELECT setval(pg_get_serial_sequence('categories', 'id'), %s, true)",
                (max(c[0] for c in CATEGORIES),))


def insert_settings(cur) -> None:
    for key, value in APP_SETTINGS.items():
        cur.execute(
            """
            INSERT INTO app_settings(key, value, updated_at)
            VALUES (%s, %s, CURRENT_TIMESTAMP)
            ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_at = CURRENT_TIMESTAMP
            """,
            (key, value),
        )


def insert_holidays(cur) -> None:
    for d, name, local in HOLIDAYS_BW:
        cur.execute(
            """
            INSERT INTO holidays(holiday_date, name, year, is_auto, local_name)
            VALUES (%s, %s, %s, TRUE, %s)
            ON CONFLICT (holiday_date) DO NOTHING
            """,
            (d, name, d.year, local),
        )


def insert_users(cur) -> None:
    for persona in PERSONAS:
        hash_value = ARGON2.hash(persona.password)
        # CHECK constraint users_admin_only_no_tracks_time enforces:
        #   tracks_time = TRUE OR role = 'admin'
        cur.execute(
            """
            INSERT INTO users(email, password_hash, first_name, last_name, role,
                              weekly_hours, workdays_per_week, start_date, active,
                              must_change_password, allow_reopen_without_approval,
                              dark_mode, overtime_start_balance_min, tracks_time,
                              created_at)
            VALUES (%s, %s, %s, %s, %s,
                    %s, %s, %s, TRUE,
                    %s, %s,
                    FALSE, %s, %s,
                    %s)
            RETURNING id
            """,
            (
                persona.email,
                hash_value,
                persona.first_name,
                persona.last_name,
                persona.role,
                persona.weekly_hours,
                persona.workdays_per_week,
                persona.start_date,
                persona.must_change_password,
                persona.allow_reopen_without_approval,
                persona.overtime_start_balance_min,
                persona.tracks_time,
                datetime.combine(persona.start_date, time(8, 0), tzinfo=timezone.utc),
            ),
        )
        persona.user_id = cur.fetchone()[0]


def persona_by_key(key: str) -> Persona:
    for p in PERSONAS:
        if p.key == key:
            return p
    raise KeyError(key)


def insert_user_approvers(cur) -> None:
    # Tabea → Arnold; Eva → Tabea; Alina → Tabea.
    # (Admins are implicit approvers via is_admin_role; user_approvers controls
    # which approvers receive notifications and appear as the "primary" reviewer
    # in the UI.)
    mapping = [
        ("team_lead", "admin"),
        ("employee",  "team_lead"),
        ("assistant", "team_lead"),
    ]
    for child_key, parent_key in mapping:
        cur.execute(
            "INSERT INTO user_approvers(user_id, approver_id) VALUES (%s, %s)",
            (persona_by_key(child_key).user_id, persona_by_key(parent_key).user_id),
        )


def insert_annual_leave(cur) -> None:
    for persona in PERSONAS:
        for year, days in sorted(persona.annual_leave_days.items()):
            cur.execute(
                """
                INSERT INTO user_annual_leave(user_id, year, days)
                VALUES (%s, %s, %s)
                """,
                (persona.user_id, year, days),
            )


def absence_timestamps(
    kind: str, start: date, status: str,
) -> tuple[datetime, datetime | None]:
    """Return (created_at, reviewed_at) tuples that are *plausible* for the
    absence kind, status, and how far in the future the absence sits.

    Real-world rules we enforce:
      * sick absences are reported on or shortly after the start date — the
        approver acts the same day or the next morning.  An approval timestamp
        BEFORE the start date is nonsense.
      * planned absences (vacation/training/special_leave/unpaid/general/
        flextime_reduction) are requested days or weeks in advance and reviewed
        before they start.
      * `cancellation_pending` stores the *original* approval timestamp — the
        cancellation request itself has not yet been actioned.
      * Neither created_at nor reviewed_at may be in the future: an operator
        on TODAY cannot have requested or approved anything from a date that
        has not happened yet.  For future absences we anchor both timestamps
        to a recent past window relative to TODAY.
    """
    today_dt = datetime.combine(TODAY, time(12, 0), tzinfo=timezone.utc)

    if kind == "sick":
        # Sick absences are always in the past (you don't pre-plan illness).
        created_at = datetime.combine(start, time(7, 30), tzinfo=timezone.utc)
        if status in ("approved", "rejected"):
            reviewed_at: datetime | None = datetime.combine(start, time(11, 0), tzinfo=timezone.utc)
        elif status == "cancelled":
            reviewed_at = datetime.combine(start, time(15, 0), tzinfo=timezone.utc)
        else:
            reviewed_at = None
        return created_at, reviewed_at

    # Future-anchored: timestamps relative to TODAY, not start_date.  Pattern:
    # the operator filed the request recently and (if applicable) it was
    # reviewed recently too.  Created always precedes reviewed.
    if start > TODAY:
        if status == "requested":
            return today_dt - timedelta(days=5), None
        if status == "approved":
            return today_dt - timedelta(days=14), today_dt - timedelta(days=7)
        if status == "rejected":
            return today_dt - timedelta(days=14), today_dt - timedelta(days=10)
        if status == "cancelled":
            return today_dt - timedelta(days=14), today_dt - timedelta(days=3)
        if status == "cancellation_pending":
            # Original request placed 30 days ago, originally approved 2 weeks
            # ago, cancellation now sitting in approver's inbox.
            return today_dt - timedelta(days=30), today_dt - timedelta(days=14)
        return today_dt - timedelta(days=5), None

    # Past-anchored: planned absences requested in advance and reviewed before
    # they started.  cancellation_pending records were created earliest
    # because they have already been approved AND a cancellation request has
    # been filed on top — three discrete events on the same row.
    if status == "cancellation_pending":
        created_at = datetime.combine(start - timedelta(days=30), time(9, 0), tzinfo=timezone.utc)
    else:
        created_at = datetime.combine(start - timedelta(days=10), time(9, 0), tzinfo=timezone.utc)
    if status == "approved":
        reviewed_at = datetime.combine(start - timedelta(days=2), time(16, 30), tzinfo=timezone.utc)
    elif status == "rejected":
        reviewed_at = datetime.combine(start - timedelta(days=3), time(11, 0), tzinfo=timezone.utc)
    elif status == "cancelled":
        reviewed_at = datetime.combine(start - timedelta(days=5), time(11, 0), tzinfo=timezone.utc)
    elif status == "cancellation_pending":
        # Original approval — 14 days before start, 16 days after creation.
        reviewed_at = datetime.combine(start - timedelta(days=14), time(16, 0), tzinfo=timezone.utc)
    else:  # requested → still pending
        reviewed_at = None
    return created_at, reviewed_at


def insert_absences(cur) -> None:
    """Insert all absences from ABSENCE_SCRIPT with kind-aware timestamps and
    role-aware reviewer attribution."""
    for persona_key, kind, start, end, status, comment in ABSENCE_SCRIPT:
        persona = persona_by_key(persona_key)
        reviewed_by: int | None = None
        rejection_reason: str | None = None

        # Reviewer attribution.
        if status in ("approved", "rejected", "cancelled", "cancellation_pending"):
            if persona.role == "team_lead":
                # Tabea's absences can only be reviewed by an admin.
                reviewer = persona_by_key("admin")
            else:
                # Eva / Alina: 80 % Tabea, 20 % Arnold.
                reviewer = persona_by_key("team_lead") if RNG.random() < 0.8 else persona_by_key("admin")
            reviewed_by = reviewer.user_id
            if status == "rejected":
                rejection_reason = "Im aktuellen Quartal leider kein Budget."

        created_at, reviewed_at = absence_timestamps(kind, start, status)
        cur.execute(
            """
            INSERT INTO absences(user_id, kind, start_date, end_date, comment,
                                 status, reviewed_by, reviewed_at, rejection_reason,
                                 created_at)
            VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s, %s)
            """,
            (
                persona.user_id, kind, start, end, comment,
                status, reviewed_by, reviewed_at, rejection_reason,
                created_at,
            ),
        )


def absence_blocked_days(persona_key: str) -> set[date]:
    """All days on which a persona has an approved/requested/pending absence
    (so no time entry should be created)."""
    blocked: set[date] = set()
    for pk, _kind, start, end, status, _comment in ABSENCE_SCRIPT:
        if pk != persona_key:
            continue
        # Cancelled absences are no-ops; rejected absences must not block work.
        if status in ("cancelled", "rejected"):
            continue
        for d in iter_dates(start, end):
            blocked.add(d)
    return blocked


# ---------------------------------------------------------------------------
# Time-entry generation per persona
# ---------------------------------------------------------------------------

def generate_eva_entries(
    cur, persona: Persona, last_complete_week_monday: date, current_week_monday: date,
) -> None:
    blocked = absence_blocked_days(persona.key)
    # One reviewer per week, not per day — the real workflow is "approver
    # batch-approves all of week X" so mixed reviewers within a week look
    # wrong in the UI.  Cache the per-week choice here.
    weekly_reviewer: dict[date, Persona] = {}
    for d in iter_dates(persona.start_date, TODAY):
        if not is_contract_workday(persona, d):
            continue
        if d in HOLIDAY_DATES:
            continue
        if d in blocked:
            continue
        pattern = pick_day_pattern(persona, d, RNG)
        week_mon = week_monday(d)
        if week_mon == current_week_monday:
            status = "draft"
        elif week_mon == last_complete_week_monday:
            status = "submitted"
        else:
            status = "approved"
        if week_mon not in weekly_reviewer:
            weekly_reviewer[week_mon] = pick_employee_approver(persona, week_mon)
        approver = weekly_reviewer[week_mon]
        write_pattern(cur, persona, d, pattern, status, approver)


def generate_tabea_entries(
    cur, persona: Persona, last_complete_week_monday: date, current_week_monday: date,
) -> None:
    blocked = absence_blocked_days(persona.key)
    for d in iter_dates(persona.start_date, TODAY):
        if not is_contract_workday(persona, d):
            continue
        if d in HOLIDAY_DATES:
            continue
        if d in blocked:
            continue
        pattern = pick_day_pattern(persona, d, RNG)
        week_mon = week_monday(d)
        if week_mon == current_week_monday:
            status = "draft"
        elif week_mon == last_complete_week_monday:
            status = "submitted"
        else:
            status = "approved"
        # Tabea is approved by Arnold (admin) — always.
        approver = persona_by_key("admin")
        write_pattern(cur, persona, d, pattern, status, approver)


def generate_alina_entries(
    cur, persona: Persona, last_complete_week_monday: date, current_week_monday: date,
) -> None:
    """Alina is the on-call springer.  She works on Eva's absence days (and
    only those), capped at ~40 hours per month and trending lower."""
    blocked_self = absence_blocked_days(persona.key)
    eva_blocked = absence_blocked_days("employee")
    # Days where Eva is absent AND it's a regular workday AND not a holiday →
    # Alina jumps in.  Not every absence day is covered (some are short-notice
    # and went uncovered); we keep coverage realistic.
    candidate_days = sorted(
        d for d in eva_blocked
        if persona.start_date <= d <= TODAY
        and d not in HOLIDAY_DATES
        and d not in blocked_self
        and d.weekday() < 5
    )
    # Approximate: Alina covers 60 % of Eva's missing workdays.
    selected = [d for d in candidate_days if RNG.random() < 0.6]
    # Add a couple of stand-alone "spontaneous" half-days even when Eva is in.
    extra_days_pool = [
        d for d in iter_dates(persona.start_date, TODAY)
        if d.weekday() in (1, 3)  # Tue/Thu
        and d not in HOLIDAY_DATES
        and d not in blocked_self
        and d not in eva_blocked
    ]
    RNG.shuffle(extra_days_pool)
    selected.extend(extra_days_pool[:6])
    selected = sorted(set(selected))

    # Enforce a HARD cap of 40h/month by tracking accumulated minutes per
    # month.  If neither the picked shift nor the half-shift fallback fits in
    # the remaining budget, skip the day entirely.
    minutes_per_month: dict[tuple[int, int], int] = {}
    MAX_MIN_PER_MONTH = 40 * 60
    MIN_SHIFT_MIN = 4 * 60
    weekly_reviewer: dict[date, Persona] = {}
    for d in selected:
        month_key = (d.year, d.month)
        used = minutes_per_month.get(month_key, 0)
        remaining = MAX_MIN_PER_MONTH - used
        if remaining < MIN_SHIFT_MIN:
            continue  # not enough budget left this month for even a half-shift
        # Pick a half-day or full-day pattern, biased toward shorter shifts.
        choice = RNG.random()
        if choice < 0.5:
            pattern = [("08:00", "12:00", CAT_WORK)]  # 4h
            shift_min = 4 * 60
        elif choice < 0.85:
            pattern = [("09:00", "13:00", CAT_WORK)]  # 4h
            shift_min = 4 * 60
        else:
            pattern = [("08:00", "12:00", CAT_WORK), ("12:30", "15:00", CAT_WORK)]  # 6.5h
            shift_min = 6 * 60 + 30
        if shift_min > remaining:
            # Fall back to the half-shift; guaranteed to fit since
            # remaining >= MIN_SHIFT_MIN by the check above.
            pattern = [("08:00", "12:00", CAT_WORK)]
            shift_min = MIN_SHIFT_MIN
        minutes_per_month[month_key] = used + shift_min

        week_mon = week_monday(d)
        if week_mon == current_week_monday:
            status = "draft"
        elif week_mon == last_complete_week_monday:
            status = "submitted"
        else:
            status = "approved"
        # One reviewer per week for Alina too (Tabea 80 % / Arnold 20 %).
        if week_mon not in weekly_reviewer:
            weekly_reviewer[week_mon] = (
                persona_by_key("team_lead") if RNG.random() < 0.8 else persona_by_key("admin")
            )
        approver = weekly_reviewer[week_mon]
        write_pattern(cur, persona, d, pattern, status, approver)


def pick_employee_approver(persona: Persona, d: date) -> Persona:
    """Eva is approved mostly by Tabea, occasionally by Arnold (admin) — for
    instance when Tabea is on vacation."""
    tabea = persona_by_key("team_lead")
    arnold = persona_by_key("admin")
    tabea_off = absence_blocked_days("team_lead")
    if d in tabea_off:
        return arnold
    # 15 % chance admin reviews directly even when Tabea is in.
    return arnold if RNG.random() < 0.15 else tabea


def write_pattern(
    cur,
    persona: Persona,
    d: date,
    pattern: list[tuple[str, str, int]],
    status: str,
    reviewer: Persona,
) -> None:
    """Insert each (start, end, category) row at the requested status with
    timestamps that match the lifecycle."""
    created_at = datetime.combine(d, time(7, 30), tzinfo=timezone.utc)
    # submitted_at: end of the week (Sun evening).  reviewed_at: Mon morning
    # of the next week.  draft entries have neither.
    week_mon = week_monday(d)
    submitted_at = datetime.combine(week_mon + timedelta(days=6), time(20, 0), tzinfo=timezone.utc)
    reviewed_at  = datetime.combine(week_mon + timedelta(days=7), time(9, 30), tzinfo=timezone.utc)

    submitted_at_val = submitted_at if status in ("submitted", "approved", "rejected") else None
    reviewed_by_val = reviewer.user_id if status in ("approved", "rejected") else None
    # updated_at must reflect the most recent state-changing write.  The app
    # bumps it on every UPDATE (draft → submit, submit → approve/reject), so
    # for seeded data it should equal the last lifecycle timestamp present.
    if status in ("approved", "rejected"):
        updated_at = reviewed_at
    elif status == "submitted":
        updated_at = submitted_at
    else:  # draft
        updated_at = created_at
    reviewed_at_val = reviewed_at if status in ("approved", "rejected") else None

    for start_s, end_s, cat_id in pattern:
        cur.execute(
            """
            INSERT INTO time_entries(user_id, entry_date, start_time, end_time,
                                     category_id, comment, status,
                                     submitted_at, reviewed_by, reviewed_at,
                                     rejection_reason, created_at, updated_at)
            VALUES (%s, %s, %s, %s, %s, NULL, %s,
                    %s, %s, %s, NULL, %s, %s)
            """,
            (
                persona.user_id, d, start_s, end_s, cat_id, status,
                submitted_at_val, reviewed_by_val, reviewed_at_val,
                created_at, updated_at,
            ),
        )


def insert_time_entries(cur) -> None:
    # Current week's Monday — entries in this week stay draft.
    current_week_mon = week_monday(TODAY)
    # The week before the current one is "submitted, awaiting approval".
    last_complete_week_mon = current_week_mon - timedelta(days=7)

    for persona in PERSONAS:
        if not persona.tracks_time:
            continue
        if persona.key == "team_lead":
            generate_tabea_entries(cur, persona, last_complete_week_mon, current_week_mon)
        elif persona.key == "employee":
            generate_eva_entries(cur, persona, last_complete_week_mon, current_week_mon)
        elif persona.key == "assistant":
            generate_alina_entries(cur, persona, last_complete_week_mon, current_week_mon)


def insert_reopen_requests(cur) -> None:
    for persona_key, week_start, status, reason, rejection_reason in REOPEN_SCRIPT:
        persona = persona_by_key(persona_key)
        # Reviewer attribution: pending requests have no reviewer yet
        # (CHECK reopen_requests_reviewed_by_pending enforces this).
        reviewer_id: int | None
        reviewed_at: datetime | None
        if status == "pending":
            reviewer_id = None
            reviewed_at = None
        else:
            if persona.role == "team_lead":
                reviewer = persona_by_key("admin")
            else:
                # One specific reopen explicitly routes to admin — narrative
                # says Tabea was on vacation when this one was reviewed.
                if week_start == date(2026, 5, 4):
                    reviewer = persona_by_key("admin")
                else:
                    reviewer = persona_by_key("team_lead")
            reviewer_id = reviewer.user_id
            reviewed_at = datetime.combine(week_start + timedelta(days=8), time(10, 0), tzinfo=timezone.utc)
        created_at = datetime.combine(week_start + timedelta(days=7), time(15, 0), tzinfo=timezone.utc)
        cur.execute(
            """
            INSERT INTO reopen_requests(user_id, week_start, reviewed_by, status,
                                        reviewed_at, rejection_reason, created_at,
                                        reason)
            VALUES (%s, %s, %s, %s, %s, %s, %s, %s)
            """,
            (
                persona.user_id, week_start, reviewer_id, status,
                reviewed_at, rejection_reason, created_at, reason,
            ),
        )


# ---------------------------------------------------------------------------
# Sanity verification
# ---------------------------------------------------------------------------

def verify(cur) -> None:
    """Final consistency checks — fail loudly if the seed violates an invariant."""
    # 1. Admin must have tracks_time=FALSE and no time entries or absences.
    cur.execute("SELECT id, tracks_time FROM users WHERE role='admin'")
    admins = cur.fetchall()
    assert len(admins) == 1, f"expected 1 admin, got {len(admins)}"
    admin_id, admin_tracks = admins[0]
    assert admin_tracks is False, "admin must have tracks_time=FALSE"
    cur.execute("SELECT COUNT(*) FROM time_entries WHERE user_id=%s", (admin_id,))
    assert cur.fetchone()[0] == 0, "admin must have no time entries"
    cur.execute("SELECT COUNT(*) FROM absences WHERE user_id=%s", (admin_id,))
    assert cur.fetchone()[0] == 0, "admin must have no absences"

    # 2. Every non-admin user must have at least one approver.
    cur.execute(
        """
        SELECT u.id FROM users u
        WHERE u.role <> 'admin'
          AND NOT EXISTS (SELECT 1 FROM user_approvers ua WHERE ua.user_id = u.id)
        """
    )
    orphans = cur.fetchall()
    assert not orphans, f"non-admin users without approvers: {orphans}"

    # 3. Every contract workday for tracking users must be either entry-covered,
    #    absence-covered, holiday or before start_date / after today.
    cur.execute(
        """
        SELECT id, start_date, workdays_per_week, role FROM users
        WHERE tracks_time AND role NOT IN ('assistant')
        """
    )
    for user_id, start_date, wd_per_week, role in cur.fetchall():
        cur.execute(
            """
            SELECT entry_date FROM time_entries WHERE user_id=%s
            """,
            (user_id,),
        )
        covered = {row[0] for row in cur.fetchall()}
        cur.execute(
            """
            SELECT start_date, end_date, status FROM absences
            WHERE user_id=%s AND status IN ('approved','requested','cancellation_pending')
            """,
            (user_id,),
        )
        absent: set[date] = set()
        for s, e, _st in cur.fetchall():
            for d in iter_dates(s, e):
                absent.add(d)
        uncovered: list[date] = []
        d = start_date
        while d <= TODAY:
            if d.weekday() < wd_per_week and d not in HOLIDAY_DATES \
                    and d not in covered and d not in absent:
                uncovered.append(d)
            d += timedelta(days=1)
        assert not uncovered, (
            f"user {user_id} ({role}) has {len(uncovered)} uncovered contract workday(s): "
            f"first={uncovered[:3]}"
        )

    # 4. Every time entry's end_time must be strictly after start_time.
    cur.execute(
        """
        SELECT id FROM time_entries
        WHERE (start_time::time) >= (end_time::time)
        """
    )
    bad = cur.fetchall()
    assert not bad, f"time entries with end<=start: {bad[:5]}"

    # 5. No two time entries of the same user/day overlap.
    cur.execute(
        """
        SELECT a.id, b.id FROM time_entries a JOIN time_entries b
          ON a.user_id = b.user_id AND a.entry_date = b.entry_date AND a.id < b.id
        WHERE (a.start_time::time, a.end_time::time)
              OVERLAPS (b.start_time::time, b.end_time::time)
        """
    )
    overlaps = cur.fetchall()
    assert not overlaps, f"overlapping time entries: {overlaps[:5]}"


# ---------------------------------------------------------------------------
# Orchestration
# ---------------------------------------------------------------------------

def main() -> int:
    parser = argparse.ArgumentParser(description="Seed the Zerf database with deterministic test data.")
    parser.add_argument("--env-file", default=str(Path(__file__).resolve().parent.parent / ".env"),
                        help="Path to the .env file (default: repo-root/.env).")
    parser.add_argument("--yes", action="store_true",
                        help="Required confirmation — this script writes to the live DB.")
    parser.add_argument("--dry-run", action="store_true",
                        help="Connect and run inside a transaction, but ROLLBACK at the end.")
    args = parser.parse_args()

    if not args.yes:
        sys.stderr.write(
            "Refusing to run without --yes.  This script writes to the live database.\n"
        )
        return 1

    env = load_environment(Path(args.env_file))

    # Verify the at-rest encryption key is present — the same key is needed by
    # the postgres container (pg_tde wrap) and the backup container (openssl).
    # The seeder itself does not use the key directly: pg_tde is transparent on
    # the SQL layer.
    if not env_value(env, "ZERF_DB_ENCRYPTION_KEY"):
        sys.stderr.write(
            "WARNING: ZERF_DB_ENCRYPTION_KEY is not set in the environment.  "
            "The seeder will still connect over plain SQL, but a real prod "
            "stack cannot start without that key.\n"
        )

    conn = connect(env)
    conn.autocommit = False
    try:
        with conn.cursor() as cur:
            # Hard guard — refuse if the admin setup flow has already created
            # a user.  No --force, no --wipe escape hatch: dropping a real
            # deployment's user must be a deliberate manual act outside this
            # script.
            assert_setup_not_complete(cur)
            clear_support_tables(cur)

            insert_categories(cur)
            insert_settings(cur)
            insert_holidays(cur)
            insert_users(cur)
            insert_user_approvers(cur)
            insert_annual_leave(cur)
            insert_absences(cur)
            insert_time_entries(cur)
            insert_reopen_requests(cur)
            verify(cur)

            # Print summary so operators can sanity-check from the wrapper output.
            cur.execute("SELECT COUNT(*) FROM users");          users_n = cur.fetchone()[0]
            cur.execute("SELECT COUNT(*) FROM time_entries");   te_n    = cur.fetchone()[0]
            cur.execute("SELECT COUNT(*) FROM absences");       abs_n   = cur.fetchone()[0]
            cur.execute("SELECT COUNT(*) FROM reopen_requests"); ro_n    = cur.fetchone()[0]
            sys.stderr.write(
                f"\n✓ Seed complete:\n"
                f"    users:           {users_n}\n"
                f"    time_entries:    {te_n}\n"
                f"    absences:        {abs_n}\n"
                f"    reopen_requests: {ro_n}\n"
            )
            sys.stderr.write("\nLogin credentials (must_change_password=FALSE):\n")
            for p in PERSONAS:
                sys.stderr.write(f"    {p.role:10s}  {p.email:55s}  password: {p.password}\n")

        if args.dry_run:
            conn.rollback()
            sys.stderr.write("\n(dry-run) ROLLBACK — no changes persisted.\n")
        else:
            conn.commit()
            sys.stderr.write("\nCOMMIT.\n")
        return 0
    except Exception as exc:
        conn.rollback()
        sys.stderr.write(f"\n✗ Seeding failed, transaction rolled back: {exc!r}\n")
        return 2
    finally:
        conn.close()


if __name__ == "__main__":
    sys.exit(main())
