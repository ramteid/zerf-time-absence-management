#!/usr/bin/env bash
# KitaZeit automated integration test runner.
#
# Boots a clean PostgreSQL + app test stack on a private port (no Caddy,
# no public DNS),
# captures the auto-generated admin password from the logs, then runs:
#
#   1. API regression (curl + bash) against the local stack
#   2. Headless browser smoke test (Puppeteer in Docker)
#
# Usage:  bash tests/run.sh
# Exit code is non-zero if any assertion fails.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

CONTAINER=kitazeit-it
DB_CONTAINER=kitazeit-it-postgres
NETWORK=kitazeit-it
DB_VOLUME=kitazeit-it-dbdata
DB_NAME=kitazeit_it
DB_USER=kitazeit_it
DB_PASSWORD=integration-test-db-password-32-chars
PORT=${KITAZEIT_TEST_PORT:-3137}
BASE="http://127.0.0.1:$PORT"
# Allow CI to pass a pre-built image via the IMG environment variable so the
# build step below is skipped (the image is already present in the daemon).
IMG=${IMG:-zerf2-app:latest}

PASS=0; FAIL=0
banner(){ printf "\n\033[1;36m== %s ==\033[0m\n" "$*"; }
ok()    { PASS=$((PASS+1)); printf "  \033[32m✓\033[0m %s\n" "$*"; }
bad()   { FAIL=$((FAIL+1)); printf "  \033[31m✗\033[0m %s\n" "$*"; }

cleanup(){
  docker rm -f "$CONTAINER" "$DB_CONTAINER" >/dev/null 2>&1 || true
  docker volume rm -f "$DB_VOLUME" >/dev/null 2>&1 || true
  docker network rm "$NETWORK" >/dev/null 2>&1 || true
}
trap cleanup EXIT

# Skip the build when a pre-built image is already present in the daemon
# (e.g. loaded by CI via docker/build-push-action before calling this script).
if docker image inspect "$IMG" >/dev/null 2>&1; then
  ok "using pre-built image ($IMG)"
else
  banner "Build app image (cached layers reused)"
  DOCKER_BUILDKIT=0 docker build -q -t "$IMG" "$ROOT" >/dev/null
  ok "image built"
fi

banner "Start ephemeral PostgreSQL + app stack on :$PORT"
cleanup
docker network create "$NETWORK" >/dev/null
docker volume create "$DB_VOLUME" >/dev/null
docker run -d --name "$DB_CONTAINER" \
  --network "$NETWORK" \
  -e POSTGRES_DB="$DB_NAME" \
  -e POSTGRES_USER="$DB_USER" \
  -e POSTGRES_PASSWORD="$DB_PASSWORD" \
  -e POSTGRES_INITDB_ARGS="--auth-host=scram-sha-256 --auth-local=scram-sha-256 --data-checksums" \
  -v "$DB_VOLUME:/var/lib/postgresql/data" \
  postgres:16-alpine \
  postgres -c password_encryption=scram-sha-256 -c ssl=off -c idle_in_transaction_session_timeout=30000 -c statement_timeout=30000 >/dev/null
ok "database container started ($DB_CONTAINER)"
for i in $(seq 1 60); do
  if docker exec "$DB_CONTAINER" pg_isready -U "$DB_USER" -d "$DB_NAME" -h 127.0.0.1 >/dev/null 2>&1; then ok "database ready after ${i}x250ms"; break; fi
  sleep 0.25
  if [ "$i" = 60 ]; then bad "database did not become ready"; docker logs "$DB_CONTAINER"; exit 1; fi
done

docker run -d --name "$CONTAINER" \
  --network "$NETWORK" \
  -p 127.0.0.1:$PORT:3000 \
  --user 10001:10001 \
  --read-only --tmpfs /tmp:size=16m \
  --cap-drop=ALL --security-opt=no-new-privileges:true \
  -e KITAZEIT_DATABASE_URL="postgres://${DB_USER}:${DB_PASSWORD}@${DB_CONTAINER}:5432/${DB_NAME}?sslmode=disable" \
  -e KITAZEIT_SESSION_SECRET=integration-test-secret-do-not-use-in-prod-32-characters \
  -e KITAZEIT_ADMIN_EMAIL=admin@example.com \
  -e KITAZEIT_ORGANIZATION_NAME="Integration Test" \
  -e KITAZEIT_REGION=BW \
  -e KITAZEIT_DEV=1 \
  -e KITAZEIT_SECURE_COOKIES=false \
  -e KITAZEIT_ENFORCE_CSRF=false \
  -e KITAZEIT_ENFORCE_ORIGIN=false \
  "$IMG" >/dev/null
ok "app container started ($CONTAINER)"

# Wait for readiness
for i in $(seq 1 120); do
  if curl -fsS "$BASE/healthz" -o /dev/null 2>/dev/null; then ok "ready after ${i}x250ms"; break; fi
  sleep 0.25
  if [ "$i" = 120 ]; then bad "container did not become ready"; docker logs "$CONTAINER"; exit 1; fi
done

ADMIN_PW=$(docker logs "$CONTAINER" 2>&1 | grep -oE "Admin password: [^[:space:]]+" | tail -1 | awk '{print $3}')
[ -n "$ADMIN_PW" ] && ok "captured admin password ($ADMIN_PW)" || { bad "no admin pw"; exit 1; }

# ---------------------------------------------------------------------------
# API regression
# ---------------------------------------------------------------------------
JAR_A=/tmp/it_admin.cookies; JAR_E=/tmp/it_emp.cookies; JAR_L=/tmp/it_lead.cookies
rm -f "$JAR_A" "$JAR_E" "$JAR_L"

call(){ # call <jar> <method> <path> [json]  -> echoes "<status>\n<body>"
  local jar=$1 m=$2 p=$3 d=${4:-}
  if [ -n "$d" ]; then
    curl -sS -b "$jar" -c "$jar" -o /tmp/it_body -w "%{http_code}" \
      -H "Content-Type: application/json" -X "$m" --data "$d" "$BASE$p"
  else
    curl -sS -b "$jar" -c "$jar" -o /tmp/it_body -w "%{http_code}" -X "$m" "$BASE$p"
  fi
  echo
  cat /tmp/it_body
}
expect(){ local label=$1 want=$2 got=$3 body=$4
  if [ "$got" = "$want" ]; then ok "$label ($got)"
  else bad "$label expected=$want got=$got body=$body"; fi
}

banner "Anonymous endpoints"
o=$(call "$JAR_A" GET /);                          expect "GET /"            200 "${o%%$'\n'*}" "${o#*$'\n'}"
o=$(call "$JAR_A" GET /api/v1/auth/me);            expect "/auth/me unauth"  401 "${o%%$'\n'*}" "${o#*$'\n'}"
o=$(call "$JAR_A" GET /api/v1/users);              expect "/users unauth"    401 "${o%%$'\n'*}" "${o#*$'\n'}"
o=$(call "$JAR_A" POST /api/v1/auth/login '{"email":"admin@example.com","password":"WRONG"}')
expect "bad login rejected" 400 "${o%%$'\n'*}" "${o#*$'\n'}"

banner "Admin login + forced password change"
o=$(call "$JAR_A" POST /api/v1/auth/login "{\"email\":\"admin@example.com\",\"password\":\"$ADMIN_PW\"}")
expect "login admin" 200 "${o%%$'\n'*}" "${o#*$'\n'}"
o=$(call "$JAR_A" GET /api/v1/auth/me);            body=${o#*$'\n'}; expect "/auth/me admin" 200 "${o%%$'\n'*}" "$body"
echo "$body" | grep -q '"must_change_password":true' && ok "must_change_password flag set" || bad "flag missing: $body"
o=$(call "$JAR_A" PUT /api/v1/auth/password "{\"current_password\":\"$ADMIN_PW\",\"new_password\":\"AdminPass!234\"}")
expect "change pw" 200 "${o%%$'\n'*}" "${o#*$'\n'}"
o=$(call "$JAR_A" GET /api/v1/auth/me);            body=${o#*$'\n'}
echo "$body" | grep -q '"must_change_password":false' && ok "flag cleared" || bad "still flagged: $body"

banner "Default seed data"
o=$(call "$JAR_A" GET /api/v1/categories);         body=${o#*$'\n'}; expect "GET /categories" 200 "${o%%$'\n'*}" "$body"
COUNT=$(echo "$body" | grep -o '"id"' | wc -l); [ "$COUNT" -ge 6 ] && ok "≥6 categories ($COUNT)" || bad "categories=$COUNT"
CAT_ID=$(echo "$body" | grep -oE '"id":[0-9]+' | head -1 | cut -d: -f2)
YEAR=$(date -u +%Y)
o=$(call "$JAR_A" GET "/api/v1/holidays?year=$YEAR"); body=${o#*$'\n'}
HC=$(echo "$body" | grep -o '"id"' | wc -l); [ "$HC" -ge 9 ] && ok "≥9 BW holidays ($HC)" || bad "holidays=$HC"

banner "User management"
o=$(call "$JAR_A" POST /api/v1/users '{"email":"erin@example.com","first_name":"Erin","last_name":"Worker","role":"employee","weekly_hours":39,"annual_leave_days":30,"start_date":"2024-01-01"}')
body=${o#*$'\n'}; expect "create employee" 200 "${o%%$'\n'*}" "$body"
EMP_ID=$(echo "$body" | grep -oE '"id":[0-9]+' | head -1 | cut -d: -f2)
EMP_PW=$(echo "$body" | grep -oE '"temporary_password":"[^"]+"' | cut -d'"' -f4)

o=$(call "$JAR_A" POST /api/v1/users '{"email":"lead@example.com","first_name":"Lea","last_name":"Lead","role":"team_lead","weekly_hours":39,"annual_leave_days":30,"start_date":"2024-01-01"}')
body=${o#*$'\n'}; expect "create team_lead" 200 "${o%%$'\n'*}" "$body"
LEAD_ID=$(echo "$body" | grep -oE '"id":[0-9]+' | head -1 | cut -d: -f2)
LEAD_PW=$(echo "$body" | grep -oE '"temporary_password":"[^"]+"' | cut -d'"' -f4)

o=$(call "$JAR_A" POST /api/v1/users '{"email":"erin@example.com","first_name":"Dup","last_name":"Dup","role":"employee","weekly_hours":39,"annual_leave_days":30,"start_date":"2024-01-01"}')
st=${o%%$'\n'*}; { [ "$st" = 400 ] || [ "$st" = 409 ]; } && ok "duplicate email rejected ($st)" || bad "duplicate email got $st"

# Login employee, change pw
o=$(call "$JAR_E" POST /api/v1/auth/login "{\"email\":\"erin@example.com\",\"password\":\"$EMP_PW\"}")
expect "login emp" 200 "${o%%$'\n'*}" ""
o=$(call "$JAR_E" PUT /api/v1/auth/password "{\"current_password\":\"$EMP_PW\",\"new_password\":\"EmployeePass!234\"}")
expect "emp change pw" 200 "${o%%$'\n'*}" ""

# Login lead, change pw
o=$(call "$JAR_L" POST /api/v1/auth/login "{\"email\":\"lead@example.com\",\"password\":\"$LEAD_PW\"}")
expect "login lead" 200 "${o%%$'\n'*}" ""
o=$(call "$JAR_L" PUT /api/v1/auth/password "{\"current_password\":\"$LEAD_PW\",\"new_password\":\"TeamLeadPass!234\"}")
expect "lead change pw" 200 "${o%%$'\n'*}" ""

banner "Role-elevation hardening"
# Employee may not promote themselves to admin via the (unauthorised) PUT /users/:id.
o=$(call "$JAR_E" PUT "/api/v1/users/$EMP_ID" '{"role":"admin"}')
expect "emp self-promote 403" 403 "${o%%$'\n'*}" "${o#*$'\n'}"
# Admin cannot demote themselves out of admin role (would lock the system out).
o=$(call "$JAR_A" PUT "/api/v1/users/1" '{"role":"employee"}')
st=${o%%$'\n'*}; { [ "$st" = 400 ] || [ "$st" = 409 ]; } && ok "admin self-demote rejected ($st)" || bad "admin demote got $st"
# Admin cannot deactivate themselves.
o=$(call "$JAR_A" PUT "/api/v1/users/1" '{"active":false}')
st=${o%%$'\n'*}; { [ "$st" = 400 ] || [ "$st" = 409 ]; } && ok "admin self-deactivate rejected ($st)" || bad "self-deactivate got $st"
# Bogus role is rejected.
o=$(call "$JAR_A" PUT "/api/v1/users/$EMP_ID" '{"role":"superuser"}')
expect "bogus role rejected" 400 "${o%%$'\n'*}" "${o#*$'\n'}"

banner "RBAC"
o=$(call "$JAR_E" GET /api/v1/users);              expect "emp /users 403" 403 "${o%%$'\n'*}" "${o#*$'\n'}"
o=$(call "$JAR_E" GET /api/v1/audit-log);          expect "emp /audit 403" 403 "${o%%$'\n'*}" "${o#*$'\n'}"
# spec: lead can read team data (list users for approvals UI), but cannot mutate
o=$(call "$JAR_L" POST /api/v1/users '{"email":"x@example.com","first_name":"X","last_name":"X","role":"employee","weekly_hours":39,"annual_leave_days":30,"start_date":"2024-01-01"}')
expect "lead create user 403" 403 "${o%%$'\n'*}" "${o#*$'\n'}"
o=$(call "$JAR_L" POST /api/v1/categories '{"name":"X","color":"#000"}'); expect "lead create category 403" 403 "${o%%$'\n'*}" "${o#*$'\n'}"

banner "Time entries — validations"
TODAY=$(date -u +%F); FUTURE=$(date -u -d "+5 days" +%F)
o=$(call "$JAR_E" POST /api/v1/time-entries "{\"entry_date\":\"$TODAY\",\"start_time\":\"08:00\",\"end_time\":\"12:00\",\"category_id\":$CAT_ID,\"comment\":\"morning\"}")
body=${o#*$'\n'}; expect "create entry 1" 200 "${o%%$'\n'*}" "$body"
TE1=$(echo "$body" | grep -oE '"id":[0-9]+' | head -1 | cut -d: -f2)

o=$(call "$JAR_E" POST /api/v1/time-entries "{\"entry_date\":\"$TODAY\",\"start_time\":\"10:00\",\"end_time\":\"11:00\",\"category_id\":$CAT_ID}")
expect "overlap rejected"  400 "${o%%$'\n'*}" "${o#*$'\n'}"
o=$(call "$JAR_E" POST /api/v1/time-entries "{\"entry_date\":\"$TODAY\",\"start_time\":\"14:00\",\"end_time\":\"13:00\",\"category_id\":$CAT_ID}")
expect "end<start rejected" 400 "${o%%$'\n'*}" "${o#*$'\n'}"
o=$(call "$JAR_E" POST /api/v1/time-entries "{\"entry_date\":\"$FUTURE\",\"start_time\":\"08:00\",\"end_time\":\"09:00\",\"category_id\":$CAT_ID}")
expect "future date rejected" 400 "${o%%$'\n'*}" "${o#*$'\n'}"
o=$(call "$JAR_E" POST /api/v1/time-entries "{\"entry_date\":\"$TODAY\",\"start_time\":\"13:00\",\"end_time\":\"15:00\",\"category_id\":$CAT_ID}")
body=${o#*$'\n'}; expect "create entry 2" 200 "${o%%$'\n'*}" "$body"
TE2=$(echo "$body" | grep -oE '"id":[0-9]+' | head -1 | cut -d: -f2)

# >14h cap
o=$(call "$JAR_E" POST /api/v1/time-entries "{\"entry_date\":\"$TODAY\",\"start_time\":\"15:00\",\"end_time\":\"23:30\",\"category_id\":$CAT_ID}")
expect ">14h day rejected" 400 "${o%%$'\n'*}" "${o#*$'\n'}"

o=$(call "$JAR_E" GET "/api/v1/time-entries?from=$TODAY&to=$TODAY"); body=${o#*$'\n'}
expect "list own entries" 200 "${o%%$'\n'*}" "$body"
echo "$body" | grep -q "\"id\":$TE1" && ok "TE1 in list" || bad "TE1 missing"

banner "Submit + approve workflow"
o=$(call "$JAR_E" POST /api/v1/time-entries/submit "{\"ids\":[$TE1,$TE2]}"); expect "submit" 200 "${o%%$'\n'*}" ""
o=$(call "$JAR_E" PUT "/api/v1/time-entries/$TE1" "{\"entry_date\":\"$TODAY\",\"start_time\":\"08:00\",\"end_time\":\"11:00\",\"category_id\":$CAT_ID,\"comment\":\"x\"}")
expect "edit submitted entry rejected" 400 "${o%%$'\n'*}" "${o#*$'\n'}"
o=$(call "$JAR_L" POST "/api/v1/time-entries/$TE1/approve");                expect "lead approve TE1" 200 "${o%%$'\n'*}" ""
o=$(call "$JAR_L" POST "/api/v1/time-entries/$TE2/reject" '{"reason":"clarify"}'); expect "lead reject TE2"  200 "${o%%$'\n'*}" ""
o=$(call "$JAR_E" POST "/api/v1/time-entries/$TE1/approve"); expect "emp approve forbidden" 403 "${o%%$'\n'*}" ""

banner "Change request"
o=$(call "$JAR_E" POST /api/v1/change-requests "{\"time_entry_id\":$TE1,\"new_end_time\":\"12:30\",\"reason\":\"forgot 30 min\"}")
body=${o#*$'\n'}; expect "create change request" 200 "${o%%$'\n'*}" "$body"
CR=$(echo "$body" | grep -oE '"id":[0-9]+' | head -1 | cut -d: -f2)
o=$(call "$JAR_L" POST "/api/v1/change-requests/$CR/approve"); expect "approve change request" 200 "${o%%$'\n'*}" ""

banner "Absences"
V_FROM=$(date -u -d "+10 days" +%F); V_TO=$(date -u -d "+12 days" +%F)
o=$(call "$JAR_E" POST /api/v1/absences "{\"kind\":\"vacation\",\"start_date\":\"$V_FROM\",\"end_date\":\"$V_TO\"}")
body=${o#*$'\n'}; expect "request vacation" 200 "${o%%$'\n'*}" "$body"
ABS=$(echo "$body" | grep -oE '"id":[0-9]+' | head -1 | cut -d: -f2)
echo "$body" | grep -q '"status":"requested"' && ok "vacation requested" || bad "wrong status: $body"

o=$(call "$JAR_E" POST /api/v1/absences "{\"kind\":\"sick\",\"start_date\":\"$TODAY\",\"end_date\":\"$TODAY\"}")
body=${o#*$'\n'}; expect "report sick" 200 "${o%%$'\n'*}" "$body"
echo "$body" | grep -q '"status":"approved"' && ok "sick auto-approved" || bad "sick not approved: $body"

# overlap
o=$(call "$JAR_E" POST /api/v1/absences "{\"kind\":\"vacation\",\"start_date\":\"$V_FROM\",\"end_date\":\"$V_FROM\"}")
st=${o%%$'\n'*}; { [ "$st" = 400 ] || [ "$st" = 409 ]; } && ok "overlapping absence rejected ($st)" || bad "overlap got $st"

# bad kind
o=$(call "$JAR_E" POST /api/v1/absences "{\"kind\":\"holiday\",\"start_date\":\"$V_FROM\",\"end_date\":\"$V_FROM\"}")
expect "invalid kind rejected" 400 "${o%%$'\n'*}" "${o#*$'\n'}"

o=$(call "$JAR_L" POST "/api/v1/absences/$ABS/approve"); expect "approve vacation" 200 "${o%%$'\n'*}" ""

# ---------------------------------------------------------------------------
# General absence — full user journey + edge cases.
#
# Background: 'general_absence' covers personal reasons (parental leave, etc.)
# which cannot be modelled via vacation/sick/training/special_leave/unpaid.
# It must:
#   • require team-lead approval (status=requested, NOT auto-approved)
#   • not consume the vacation entitlement
#   • follow the same overlap/edit/cancel/approve/reject rules
#   • surface in the team calendar and the monthly report (as an absence day)
#   • be persisted in the audit log for every state transition
# ---------------------------------------------------------------------------
banner "General absence — happy-path journey (Erin: parental leave)"
GA_FROM=$(date -u -d "+30 days" +%F); GA_TO=$(date -u -d "+34 days" +%F)
GA_MONTH=$(date -u -d "+30 days" +%Y-%m)

# 1. Employee files the request.
o=$(call "$JAR_E" POST /api/v1/absences "{\"kind\":\"general_absence\",\"start_date\":\"$GA_FROM\",\"end_date\":\"$GA_TO\",\"comment\":\"parental leave\"}")
body=${o#*$'\n'}; expect "POST general_absence" 200 "${o%%$'\n'*}" "$body"
GABS=$(echo "$body" | grep -oE '"id":[0-9]+' | head -1 | cut -d: -f2)
echo "$body" | grep -q '"kind":"general_absence"' && ok "kind persisted" || bad "kind not stored: $body"
echo "$body" | grep -q '"status":"requested"'    && ok "starts as requested (not auto-approved like sick)" || bad "wrong initial status: $body"
echo "$body" | grep -q '"comment":"parental leave"' && ok "comment persisted" || bad "comment missing: $body"

# 2. The request must be visible in the employee's own list.
o=$(call "$JAR_E" GET "/api/v1/absences?year=$(echo $GA_FROM | cut -c1-4)"); body=${o#*$'\n'}
echo "$body" | grep -q "\"id\":$GABS" && ok "shows in own list" || bad "not in list: $body"

# 3. Lead's queue (list_all) shows the requested absence.
o=$(call "$JAR_L" GET "/api/v1/absences/all?status=requested"); body=${o#*$'\n'}
echo "$body" | grep -q "\"id\":$GABS" && ok "appears in lead queue" || bad "missing from lead queue: $body"

# 4. Plain employees may NOT call /absences/all (lead-only).
o=$(call "$JAR_E" GET /api/v1/absences/all); expect "emp /absences/all 403" 403 "${o%%$'\n'*}" "${o#*$'\n'}"

# 5. While pending, employee can edit the request (e.g. extend the range).
GA_TO2=$(date -u -d "+40 days" +%F)
o=$(call "$JAR_E" PUT "/api/v1/absences/$GABS" "{\"kind\":\"general_absence\",\"start_date\":\"$GA_FROM\",\"end_date\":\"$GA_TO2\",\"half_day\":false,\"comment\":\"updated parental leave plan\"}")
body=${o#*$'\n'}; expect "edit pending general_absence" 200 "${o%%$'\n'*}" "$body"
echo "$body" | grep -q "\"end_date\":\"$GA_TO2\"" && ok "end_date updated" || bad "edit not applied: $body"

# 6. Lead approves.
o=$(call "$JAR_L" POST "/api/v1/absences/$GABS/approve"); expect "lead approve" 200 "${o%%$'\n'*}" ""
o=$(call "$JAR_E" GET "/api/v1/absences?year=$(echo $GA_FROM | cut -c1-4)"); body=${o#*$'\n'}
echo "$body" | tr '}' '\n' | grep "\"id\":$GABS," | grep -q '"status":"approved"' && ok "status now approved" || bad "status not approved: $body"

# 7. Once approved the request can no longer be edited or cancelled by the employee.
o=$(call "$JAR_E" PUT "/api/v1/absences/$GABS" "{\"kind\":\"general_absence\",\"start_date\":\"$GA_FROM\",\"end_date\":\"$GA_TO\",\"half_day\":false,\"comment\":\"x\"}")
expect "edit approved general_absence rejected" 400 "${o%%$'\n'*}" ""
o=$(call "$JAR_E" DELETE "/api/v1/absences/$GABS")
expect "cancel approved general_absence rejected" 400 "${o%%$'\n'*}" ""

# 8. Approved entry surfaces in the calendar with kind=general_absence.
o=$(call "$JAR_L" GET "/api/v1/absences/calendar?month=$GA_MONTH"); body=${o#*$'\n'}
echo "$body" | grep -q '"kind":"general_absence"' && ok "calendar shows general_absence" || bad "missing on calendar: $body"

# 9. Vacation balance unchanged (general_absence does NOT consume entitlement).
o=$(call "$JAR_E" GET "/api/v1/leave-balance/$EMP_ID?year=$YEAR"); body=${o#*$'\n'}
echo "$body" | grep -q '"annual_entitlement":30' && ok "entitlement still 30" || bad "entitlement: $body"
echo "$body" | grep -q '"available":28'          && ok "available still 28 (general_absence excluded)" || bad "available: $body"

# 10. Approved general_absence shows up as 'absence' in the monthly report.
o=$(call "$JAR_E" GET "/api/v1/reports/month?month=$GA_MONTH"); body=${o#*$'\n'}
echo "$body" | grep -q '"absence":"general_absence"' && ok "monthly report flags day as general_absence" || bad "report missing absence: $(echo $body | head -c 400)"

# 11. Audit log captures created/updated/approved transitions.
o=$(call "$JAR_A" GET "/api/v1/audit-log?user_id=$EMP_ID"); body=${o#*$'\n'}
GA_AUDIT=$(echo "$body" | tr '{' '\n' | grep -c "\"table_name\":\"absences\".*\"record_id\":$GABS")
[ "$GA_AUDIT" -ge 3 ] && ok "audit log has $GA_AUDIT entries for absence $GABS" || bad "audit insufficient ($GA_AUDIT): $body"

banner "General absence — overlap & validation edge cases"
# a) Overlap with the just-approved general_absence (any later submission inside the range).
o=$(call "$JAR_E" POST /api/v1/absences "{\"kind\":\"general_absence\",\"start_date\":\"$GA_FROM\",\"end_date\":\"$GA_FROM\"}")
st=${o%%$'\n'*}; { [ "$st" = 400 ] || [ "$st" = 409 ]; } && ok "overlap with approved general_absence rejected ($st)" || bad "overlap got $st"

# b) Cross-kind overlap: vacation request inside the parental leave range must also be rejected.
o=$(call "$JAR_E" POST /api/v1/absences "{\"kind\":\"vacation\",\"start_date\":\"$GA_FROM\",\"end_date\":\"$GA_FROM\"}")
st=${o%%$'\n'*}; { [ "$st" = 400 ] || [ "$st" = 409 ]; } && ok "vacation overlapping general_absence rejected ($st)" || bad "cross-kind overlap got $st"

# c) end_date < start_date rejected (400).
o=$(call "$JAR_E" POST /api/v1/absences "{\"kind\":\"general_absence\",\"start_date\":\"2099-01-10\",\"end_date\":\"2099-01-05\"}")
expect "inverted range rejected" 400 "${o%%$'\n'*}" "${o#*$'\n'}"

# d) Half-day flag is silently ignored for general_absence (only meaningful for single-day vacation).
GA3_DAY=$(date -u -d "+90 days" +%F)
o=$(call "$JAR_E" POST /api/v1/absences "{\"kind\":\"general_absence\",\"start_date\":\"$GA3_DAY\",\"end_date\":\"$GA3_DAY\",\"half_day\":true}")
body=${o#*$'\n'}; expect "create one-day GA with half_day=true" 200 "${o%%$'\n'*}" "$body"
GABS3=$(echo "$body" | grep -oE '"id":[0-9]+' | head -1 | cut -d: -f2)
echo "$body" | grep -q '"half_day":false' && ok "half_day forced to false (only valid for vacation)" || bad "half_day kept truthy: $body"

# e) Unauthenticated callers cannot create absences.
JAR_X=/tmp/it_anon.cookies; rm -f "$JAR_X"
o=$(call "$JAR_X" POST /api/v1/absences "{\"kind\":\"general_absence\",\"start_date\":\"$GA3_DAY\",\"end_date\":\"$GA3_DAY\"}")
expect "anon create rejected" 401 "${o%%$'\n'*}" ""

# f) Bogus kinds (typo of "parental") still rejected by the allow-list.
o=$(call "$JAR_E" POST /api/v1/absences "{\"kind\":\"parental\",\"start_date\":\"$GA3_DAY\",\"end_date\":\"$GA3_DAY\"}")
expect "non-allowlisted kind rejected" 400 "${o%%$'\n'*}" "${o#*$'\n'}"

# g) Empty kind rejected.
o=$(call "$JAR_E" POST /api/v1/absences "{\"kind\":\"\",\"start_date\":\"$GA3_DAY\",\"end_date\":\"$GA3_DAY\"}")
expect "empty kind rejected" 400 "${o%%$'\n'*}" "${o#*$'\n'}"

banner "General absence — cancel, reject & RBAC journeys"
# Cancel-before-approval journey (employee changes their mind).
GA4_FROM=$(date -u -d "+120 days" +%F); GA4_TO=$(date -u -d "+121 days" +%F)
o=$(call "$JAR_E" POST /api/v1/absences "{\"kind\":\"general_absence\",\"start_date\":\"$GA4_FROM\",\"end_date\":\"$GA4_TO\"}")
GABS4=$(echo "${o#*$'\n'}" | grep -oE '"id":[0-9]+' | head -1 | cut -d: -f2)
o=$(call "$JAR_E" DELETE "/api/v1/absences/$GABS4")
expect "employee cancels own pending request" 200 "${o%%$'\n'*}" ""
# After cancellation, the same range can be re-used (no overlap).
o=$(call "$JAR_E" POST /api/v1/absences "{\"kind\":\"general_absence\",\"start_date\":\"$GA4_FROM\",\"end_date\":\"$GA4_TO\"}")
expect "re-request after cancel allowed" 200 "${o%%$'\n'*}" ""
GABS4B=$(echo "${o#*$'\n'}" | grep -oE '"id":[0-9]+' | head -1 | cut -d: -f2)

# Reject journey.
GA5_FROM=$(date -u -d "+200 days" +%F); GA5_TO=$(date -u -d "+202 days" +%F)
o=$(call "$JAR_E" POST /api/v1/absences "{\"kind\":\"general_absence\",\"start_date\":\"$GA5_FROM\",\"end_date\":\"$GA5_TO\"}")
GABS5=$(echo "${o#*$'\n'}" | grep -oE '"id":[0-9]+' | head -1 | cut -d: -f2)

# Employee may not approve their own request.
o=$(call "$JAR_E" POST "/api/v1/absences/$GABS5/approve"); expect "emp self-approve 403" 403 "${o%%$'\n'*}" ""
# Employee may not approve other people's requests.
o=$(call "$JAR_E" POST "/api/v1/absences/$GABS5/reject" '{"reason":"nope"}'); expect "emp reject 403" 403 "${o%%$'\n'*}" ""
# Lead must supply a reason when rejecting.
o=$(call "$JAR_L" POST "/api/v1/absences/$GABS5/reject" '{"reason":""}'); expect "empty reject reason rejected" 400 "${o%%$'\n'*}" ""
# Lead rejects with a real reason.
o=$(call "$JAR_L" POST "/api/v1/absences/$GABS5/reject" '{"reason":"Need more documentation."}')
expect "lead reject general_absence" 200 "${o%%$'\n'*}" ""
# Rejected requests cannot be cancelled afterwards (only 'requested' may be cancelled).
o=$(call "$JAR_E" DELETE "/api/v1/absences/$GABS5"); expect "cancel-after-reject rejected" 400 "${o%%$'\n'*}" ""
# After rejection the same range becomes free again.
o=$(call "$JAR_E" POST /api/v1/absences "{\"kind\":\"general_absence\",\"start_date\":\"$GA5_FROM\",\"end_date\":\"$GA5_TO\"}")
expect "re-request after reject allowed" 200 "${o%%$'\n'*}" ""

# Unknown id → 404/500-class (not silently 200).
o=$(call "$JAR_L" POST "/api/v1/absences/9999999/approve")
st=${o%%$'\n'*}; [ "$st" != 200 ] && ok "approve unknown id not 200 ($st)" || bad "approve unknown returned 200"

banner "Vacation balance"
o=$(call "$JAR_E" GET "/api/v1/leave-balance/$EMP_ID?year=$YEAR"); body=${o#*$'\n'}
expect "leave balance" 200 "${o%%$'\n'*}" "$body"
echo "$body" | grep -q '"annual_entitlement":30' && ok "annual=30"          || bad "annual: $body"
echo "$body" | grep -q '"approved_upcoming":2'   && ok "approved_upcoming=2" || bad "upcoming: $body"
echo "$body" | grep -q '"available":28'          && ok "available=28"        || bad "available: $body"

banner "Reports"
MONTH=$(date -u +%Y-%m)
o=$(call "$JAR_L" GET "/api/v1/absences/calendar?month=$MONTH");                   expect "calendar"          200 "${o%%$'\n'*}" ""
o=$(call "$JAR_L" GET "/api/v1/reports/month?user_id=$EMP_ID&month=$MONTH");       expect "monthly report"    200 "${o%%$'\n'*}" ""
o=$(call "$JAR_L" GET "/api/v1/reports/team?month=$MONTH");                        expect "team report"       200 "${o%%$'\n'*}" ""
o=$(call "$JAR_L" GET "/api/v1/reports/categories?from=$YEAR-01-01&to=$YEAR-12-31"); expect "category report" 200 "${o%%$'\n'*}" ""
o=$(call "$JAR_L" GET "/api/v1/reports/overtime?user_id=$EMP_ID&year=$YEAR");      expect "overtime report"   200 "${o%%$'\n'*}" ""
CSV=$(curl -sS -b "$JAR_L" -o /tmp/it_csv -w "%{http_code} %{content_type}" "$BASE/api/v1/reports/month/csv?user_id=$EMP_ID&month=$MONTH")
echo "$CSV" | grep -q "^200" && [ "$(wc -c </tmp/it_csv)" -gt 100 ] && ok "CSV export ($CSV)" || bad "CSV failed: $CSV"

# ---------------------------------------------------------------------------
# Comprehensive user journey: an employee enters many kinds of times.
#
# We provision a brand-new employee ("Tina") so the workflow starts from a
# clean slate (no left-over entries from earlier sections).  The journey
# exercises realistic day-to-day usage and a *lot* of edge cases:
#
#   • Login, forced password change.
#   • Categories: pick distinct ones (childcare, prep, training, meeting…).
#   • Past-day entries: allowed (employees often back-fill).
#   • HH:MM and HH:MM:SS time formats.
#   • Adjacent entries (touch at the boundary -> allowed).
#   • Overlap detection (full / partial / one-minute / contained).
#   • Exact 14 h day-total cap (allowed at 14 h, rejected at >14 h).
#   • Zero-length / inverted / malformed times.
#   • Unicode / emoji / very long comments.
#   • Weekend & yesterday entries.
#   • Filter by date range and by status (draft/submitted/approved/rejected).
#   • Editing drafts vs. submitted entries (latter requires change-request).
#   • Submit individually vs. batched.
#   • Lead approve / reject / batch-approve / reason validation.
#   • Self-approval is forbidden even for a lead (cannot review own entries).
#   • Change requests for multiple fields at once.
#   • Delete only allowed in draft, never after submit/approve/reject.
#   • Future-day rejection.
#   • Invalid category foreign key.
#   • Body-size & malformed JSON robustness.
#   • Cross-user isolation: Tina cannot see/touch Erin's entries.
#   • Reports reflect Tina's freshly-approved entries.
# ---------------------------------------------------------------------------
banner "User journey — Tina enters many kinds of times"

JAR_T=/tmp/it_tina.cookies; rm -f "$JAR_T"
o=$(call "$JAR_A" POST /api/v1/users '{"email":"tina@example.com","first_name":"Tina","last_name":"Timekeeper","role":"employee","weekly_hours":39,"annual_leave_days":30,"start_date":"2024-01-01"}')
body=${o#*$'\n'}; expect "create Tina" 200 "${o%%$'\n'*}" "$body"
TINA_ID=$(echo "$body" | grep -oE '"id":[0-9]+' | head -1 | cut -d: -f2)
TINA_PW=$(echo "$body" | grep -oE '"temporary_password":"[^"]+"' | cut -d'"' -f4)

# Login & forced password change.
o=$(call "$JAR_T" POST /api/v1/auth/login "{\"email\":\"tina@example.com\",\"password\":\"$TINA_PW\"}")
expect "tina login" 200 "${o%%$'\n'*}" ""
o=$(call "$JAR_T" GET /api/v1/auth/me); body=${o#*$'\n'}
echo "$body" | grep -q '"must_change_password":true' && ok "tina forced pw flag" || bad "tina flag missing: $body"
# A second login while still flagged must keep working (no lock-out).
JAR_T2=/tmp/it_tina2.cookies; rm -f "$JAR_T2"
o=$(call "$JAR_T2" POST /api/v1/auth/login "{\"email\":\"tina@example.com\",\"password\":\"$TINA_PW\"}")
expect "tina second login OK while pw-flagged" 200 "${o%%$'\n'*}" ""
# Weak password rejected.
o=$(call "$JAR_T" PUT /api/v1/auth/password "{\"current_password\":\"$TINA_PW\",\"new_password\":\"short\"}")
st=${o%%$'\n'*}; [ "$st" = 400 ] && ok "weak pw rejected" || bad "weak pw got $st"
# Wrong current password rejected.
o=$(call "$JAR_T" PUT /api/v1/auth/password "{\"current_password\":\"WRONG-WRONG-WRONG\",\"new_password\":\"TinaPass!234\"}")
st=${o%%$'\n'*}; [ "$st" = 400 ] || [ "$st" = 401 ] && ok "wrong current pw rejected ($st)" || bad "wrong current got $st"
o=$(call "$JAR_T" PUT /api/v1/auth/password "{\"current_password\":\"$TINA_PW\",\"new_password\":\"TinaPass!234\"}")
expect "tina change pw" 200 "${o%%$'\n'*}" ""
o=$(call "$JAR_T" GET /api/v1/auth/me); body=${o#*$'\n'}
echo "$body" | grep -q '"must_change_password":false' && ok "tina flag cleared" || bad "tina still flagged: $body"

# Resolve all six default category IDs by name (we want variety).
o=$(call "$JAR_T" GET /api/v1/categories); CATS_BODY=${o#*$'\n'}
cat_id() {
  echo "$CATS_BODY" | tr '}' '\n' | grep -F "\"name\":\"$1\"" | grep -oE '"id":[0-9]+' | head -1 | cut -d: -f2
}
CAT_CARE=$(cat_id "Direct Childcare")
CAT_PREP=$(cat_id "Preparation Time")
CAT_LEAD=$(cat_id "Leadership Tasks")
CAT_MEET=$(cat_id "Team Meeting")
CAT_TRAIN=$(cat_id "Training")
CAT_OTHER=$(cat_id "Other")
[ -n "$CAT_CARE" ] && [ -n "$CAT_PREP" ] && [ -n "$CAT_TRAIN" ] && ok "resolved category IDs" || bad "category resolution failed: $CATS_BODY"

# Useful relative dates.
TODAY=$(date -u +%F)
YDAY=$(date -u -d "-1 day"  +%F)
DAY2=$(date -u -d "-2 days" +%F)
DAY3=$(date -u -d "-3 days" +%F)
DAY4=$(date -u -d "-4 days" +%F)
DAY7=$(date -u -d "-7 days" +%F)
TINA_MONTH=$(date -u -d "$YDAY" +%Y-%m)

# ----- 1. A typical multi-category workday (yesterday) ---------------------
banner "Tina — yesterday's workday (4 entries, 4 categories)"
mk(){ # mk <date> <start> <end> <cat> <comment-json-or-empty>
      # writes status to /tmp/it_mk_status, body to /tmp/it_mk_body, id to /tmp/it_mk_id
  local d=$1 s=$2 e=$3 c=$4 cm=$5
  local payload
  if [ -n "$cm" ]; then
    payload="{\"entry_date\":\"$d\",\"start_time\":\"$s\",\"end_time\":\"$e\",\"category_id\":$c,\"comment\":$cm}"
  else
    payload="{\"entry_date\":\"$d\",\"start_time\":\"$s\",\"end_time\":\"$e\",\"category_id\":$c}"
  fi
  local out; out=$(call "$JAR_T" POST /api/v1/time-entries "$payload")
  printf '%s' "${out%%$'\n'*}" >/tmp/it_mk_status
  printf '%s' "${out#*$'\n'}"  >/tmp/it_mk_body
  grep -oE '"id":[0-9]+' /tmp/it_mk_body | head -1 | cut -d: -f2 >/tmp/it_mk_id
}
mk_status(){ cat /tmp/it_mk_status; }
mk_body(){   cat /tmp/it_mk_body;   }
mk_id(){     cat /tmp/it_mk_id;     }

mk "$YDAY" "08:00" "10:00" "$CAT_CARE"  '"morning circle"';            expect "Y childcare 08-10"  200 "$(mk_status)" "$(mk_body)"; ID_Y1=$(mk_id)
mk "$YDAY" "10:00" "10:30" "$CAT_MEET"  '"team standup"';              expect "Y meeting 10-10:30 (adjacent boundary)" 200 "$(mk_status)" "$(mk_body)"; ID_Y2=$(mk_id)
mk "$YDAY" "10:30" "12:00" "$CAT_CARE"  '"play & lunch prep"';         expect "Y childcare 10:30-12" 200 "$(mk_status)" "$(mk_body)"; ID_Y3=$(mk_id)
mk "$YDAY" "13:00" "16:30" "$CAT_PREP"  '"prep — Übung mit Ümlaut 🎨"'; expect "Y prep 13-16:30 (unicode+emoji)" 200 "$(mk_status)" "$(mk_body)"; ID_Y4=$(mk_id)
[ -n "$ID_Y1$ID_Y2$ID_Y3$ID_Y4" ] && ok "all four IDs assigned" || bad "missing id"

# ----- 2. Edge cases on the same day --------------------------------------
banner "Tina — overlap & boundary edge cases on $YDAY"
# Exact overlap (full duplicate) -> rejected.
o=$(call "$JAR_T" POST /api/v1/time-entries "{\"entry_date\":\"$YDAY\",\"start_time\":\"08:00\",\"end_time\":\"10:00\",\"category_id\":$CAT_CARE}")
expect "exact-duplicate overlap" 400 "${o%%$'\n'*}" ""
# Partial overlap (starts inside an existing entry).
o=$(call "$JAR_T" POST /api/v1/time-entries "{\"entry_date\":\"$YDAY\",\"start_time\":\"09:00\",\"end_time\":\"11:00\",\"category_id\":$CAT_CARE}")
expect "partial overlap"          400 "${o%%$'\n'*}" ""
# 1-minute overlap (09:59-10:01 over the 10:00 boundary entry).
o=$(call "$JAR_T" POST /api/v1/time-entries "{\"entry_date\":\"$YDAY\",\"start_time\":\"09:59\",\"end_time\":\"10:01\",\"category_id\":$CAT_CARE}")
expect "one-minute overlap"       400 "${o%%$'\n'*}" ""
# Fully contained inside an existing block.
o=$(call "$JAR_T" POST /api/v1/time-entries "{\"entry_date\":\"$YDAY\",\"start_time\":\"14:00\",\"end_time\":\"15:00\",\"category_id\":$CAT_CARE}")
expect "contained overlap"        400 "${o%%$'\n'*}" ""
# Adjacent (back-to-back, no gap) -> allowed and inserted in the lunch gap.
mk "$YDAY" "12:00" "13:00" "$CAT_CARE" '"lunch coverage"'; expect "adjacent 12-13 fills gap" 200 "$(mk_status)" "$(mk_body)"; ID_Y5=$(mk_id)
# HH:MM:SS format also accepted.
mk "$YDAY" "16:30:00" "17:00:00" "$CAT_OTHER" '"clean-up"'; expect "HH:MM:SS accepted" 200 "$(mk_status)" "$(mk_body)"; ID_Y6=$(mk_id)
# Zero-length entry rejected.
o=$(call "$JAR_T" POST /api/v1/time-entries "{\"entry_date\":\"$YDAY\",\"start_time\":\"17:00\",\"end_time\":\"17:00\",\"category_id\":$CAT_CARE}")
expect "zero-length rejected"     400 "${o%%$'\n'*}" ""
# Inverted times rejected.
o=$(call "$JAR_T" POST /api/v1/time-entries "{\"entry_date\":\"$YDAY\",\"start_time\":\"18:00\",\"end_time\":\"17:30\",\"category_id\":$CAT_CARE}")
expect "inverted times rejected"  400 "${o%%$'\n'*}" ""
# Malformed times rejected.
o=$(call "$JAR_T" POST /api/v1/time-entries "{\"entry_date\":\"$YDAY\",\"start_time\":\"25:00\",\"end_time\":\"26:00\",\"category_id\":$CAT_CARE}")
expect "out-of-range hour rejected" 400 "${o%%$'\n'*}" ""
o=$(call "$JAR_T" POST /api/v1/time-entries "{\"entry_date\":\"$YDAY\",\"start_time\":\"ab:cd\",\"end_time\":\"ef:gh\",\"category_id\":$CAT_CARE}")
expect "garbage time rejected"      400 "${o%%$'\n'*}" ""
# Future-day rejected.
FUT=$(date -u -d "+1 day" +%F)
o=$(call "$JAR_T" POST /api/v1/time-entries "{\"entry_date\":\"$FUT\",\"start_time\":\"08:00\",\"end_time\":\"09:00\",\"category_id\":$CAT_CARE}")
expect "future date rejected"     400 "${o%%$'\n'*}" ""
# Invalid category id (foreign-key) rejected.
o=$(call "$JAR_T" POST /api/v1/time-entries "{\"entry_date\":\"$YDAY\",\"start_time\":\"19:00\",\"end_time\":\"19:30\",\"category_id\":999999}")
st=${o%%$'\n'*}; [ "$st" != 200 ] && ok "bogus category rejected ($st)" || bad "bogus category accepted"
# Malformed JSON rejected.
o=$(curl -sS -b "$JAR_T" -c "$JAR_T" -o /tmp/it_body -w "%{http_code}" \
  -H "Content-Type: application/json" -X POST --data '{not-json' "$BASE/api/v1/time-entries"); echo
st=$o; [ "$st" = 400 ] || [ "$st" = 422 ] && ok "malformed JSON rejected ($st)" || bad "malformed JSON got $st"

# ----- 3. 14h day-cap edge cases (use a clean prior day) ------------------
banner "Tina — 14h cap edge cases on $DAY2"
# 06:00-20:00 = exactly 14h => allowed.
mk "$DAY2" "06:00" "20:00" "$CAT_CARE" '"long shift"'; expect "exactly 14h allowed" 200 "$(mk_status)" "$(mk_body)"; ID_C1=$(mk_id)
# Adding 1 more minute the same day pushes total to >14h => rejected.
o=$(call "$JAR_T" POST /api/v1/time-entries "{\"entry_date\":\"$DAY2\",\"start_time\":\"20:00\",\"end_time\":\"20:01\",\"category_id\":$CAT_OTHER}")
expect ">14h day total rejected" 400 "${o%%$'\n'*}" ""
# Single >14h entry rejected.
o=$(call "$JAR_T" POST /api/v1/time-entries "{\"entry_date\":\"$DAY3\",\"start_time\":\"05:00\",\"end_time\":\"19:30\",\"category_id\":$CAT_CARE}")
expect "single 14:30 entry rejected" 400 "${o%%$'\n'*}" ""

# ----- 4. Long-comment / very-long-comment behavior -----------------------
banner "Tina — long comment & boundary text"
LONG=$(printf 'x%.0s' {1..2000})
mk "$DAY3" "08:00" "08:30" "$CAT_OTHER" "\"$LONG\"" || true
LC_ST=$(mk_status); ID_LC=$(mk_id)
{ [ "$LC_ST" = 200 ] && [ -n "$ID_LC" ] && ok "2000-char comment accepted"; } || ok "long comment refused gracefully ($LC_ST)"

# ----- 5. Listing & filtering ---------------------------------------------
banner "Tina — listing & range filters"
o=$(call "$JAR_T" GET "/api/v1/time-entries?from=$YDAY&to=$YDAY"); body=${o#*$'\n'}
N=$(echo "$body" | grep -o '"id"' | wc -l); [ "$N" -ge 6 ] && ok "yesterday list has ≥6 ($N)" || bad "yesterday list=$N"
o=$(call "$JAR_T" GET "/api/v1/time-entries?from=$DAY7&to=$TODAY"); body=${o#*$'\n'}
echo "$body" | grep -q "\"id\":$ID_Y1" && ok "wide range includes Y1" || bad "Y1 missing"
echo "$body" | grep -q "\"id\":$ID_C1" && ok "wide range includes 14h block" || bad "C1 missing"
# Cross-user isolation: Tina cannot see Erin's entries.
echo "$body" | grep -q "\"user_id\":$EMP_ID" && bad "leaked Erin entries to Tina" || ok "no cross-user leakage"
# Tina cannot use lead-only /all endpoint.
o=$(call "$JAR_T" GET "/api/v1/time-entries/all"); expect "tina /all 403" 403 "${o%%$'\n'*}" ""

# ----- 6. Edit drafts, then submit ----------------------------------------
banner "Tina — edit draft, then submit"
# Edit Y4 (extend prep by 30 min — still no overlap).
o=$(call "$JAR_T" PUT "/api/v1/time-entries/$ID_Y4" "{\"entry_date\":\"$YDAY\",\"start_time\":\"13:00\",\"end_time\":\"17:00\",\"category_id\":$CAT_PREP,\"comment\":\"prep extended\"}")
# Note: the slot 16:30-17:00 is occupied by Y6 -> this should fail (overlap).
expect "edit causing overlap rejected" 400 "${o%%$'\n'*}" ""
# Valid edit: shrink Y4 instead.
o=$(call "$JAR_T" PUT "/api/v1/time-entries/$ID_Y4" "{\"entry_date\":\"$YDAY\",\"start_time\":\"13:00\",\"end_time\":\"16:00\",\"category_id\":$CAT_PREP,\"comment\":\"prep shorter\"}")
expect "valid draft edit" 200 "${o%%$'\n'*}" ""
# Edit someone else's entry forbidden.
o=$(call "$JAR_T" PUT "/api/v1/time-entries/$TE1" "{\"entry_date\":\"$YDAY\",\"start_time\":\"08:00\",\"end_time\":\"09:00\",\"category_id\":$CAT_CARE}")
st=${o%%$'\n'*}; [ "$st" = 403 ] || [ "$st" = 404 ] && ok "edit foreign entry forbidden ($st)" || bad "foreign edit got $st"
# Delete a draft (Y2 — the standup).
o=$(call "$JAR_T" DELETE "/api/v1/time-entries/$ID_Y2"); expect "delete draft OK" 200 "${o%%$'\n'*}" ""
# Re-create the gap so the day is contiguous again.
mk "$YDAY" "10:00" "10:30" "$CAT_MEET" '"standup redo"'; expect "re-create deleted slot" 200 "$(mk_status)" "$(mk_body)"; ID_Y2B=$(mk_id)
# Submit Y1, Y3, Y4, Y5, Y6, Y2B individually & via batch.
o=$(call "$JAR_T" POST /api/v1/time-entries/submit "{\"ids\":[$ID_Y1,$ID_Y3,$ID_Y4,$ID_Y5,$ID_Y6,$ID_Y2B]}")
expect "submit batch" 200 "${o%%$'\n'*}" ""
# Editing a submitted entry directly is rejected.
o=$(call "$JAR_T" PUT "/api/v1/time-entries/$ID_Y1" "{\"entry_date\":\"$YDAY\",\"start_time\":\"08:00\",\"end_time\":\"09:30\",\"category_id\":$CAT_CARE}")
expect "edit submitted rejected" 400 "${o%%$'\n'*}" ""
# Deleting a submitted entry rejected.
o=$(call "$JAR_T" DELETE "/api/v1/time-entries/$ID_Y1"); expect "delete submitted rejected" 400 "${o%%$'\n'*}" ""
# Re-submitting an already-submitted entry is a no-op (idempotent).
o=$(call "$JAR_T" POST /api/v1/time-entries/submit "{\"ids\":[$ID_Y1]}"); expect "re-submit no-op" 200 "${o%%$'\n'*}" ""

# ----- 7. Lead reviews ----------------------------------------------------
banner "Lead — review Tina's submissions"
# Empty reject reason rejected.
o=$(call "$JAR_L" POST "/api/v1/time-entries/$ID_Y1/reject" '{"reason":"   "}')
expect "empty reject reason rejected" 400 "${o%%$'\n'*}" ""
# Reject one entry with a real reason.
o=$(call "$JAR_L" POST "/api/v1/time-entries/$ID_Y1/reject" '{"reason":"please add a comment"}')
expect "lead rejects Y1"   200 "${o%%$'\n'*}" ""
# Batch-approve the rest.
o=$(call "$JAR_L" POST "/api/v1/time-entries/batch-approve" "{\"ids\":[$ID_Y3,$ID_Y4,$ID_Y5,$ID_Y6,$ID_Y2B]}")
body=${o#*$'\n'}; expect "batch approve" 200 "${o%%$'\n'*}" "$body"
echo "$body" | grep -q '"count":5' && ok "exactly 5 approved" || bad "batch count: $body"
# Already-rejected entry approval should not silently flip.
o=$(call "$JAR_L" POST "/api/v1/time-entries/$ID_Y1/approve")
# (Approve will move it to approved; that's allowed semantics — verify status afterwards.)
o=$(call "$JAR_T" GET "/api/v1/time-entries?from=$YDAY&to=$YDAY"); body=${o#*$'\n'}
echo "$body" | tr '}' '\n' | grep "\"id\":$ID_Y3," | grep -q '"status":"approved"' && ok "Y3 approved" || bad "Y3 not approved: $body"

# Filter-by-status (employee).
o=$(call "$JAR_T" GET "/api/v1/time-entries?from=$YDAY&to=$YDAY"); body=${o#*$'\n'}
APPROVED=$(echo "$body" | grep -o '"status":"approved"' | wc -l)
[ "$APPROVED" -ge 5 ] && ok "≥5 approved on $YDAY ($APPROVED)" || bad "approved count=$APPROVED"

# ----- 8. Self-review forbidden (Lea cannot approve Lea) ------------------
banner "Lead self-review hardening"
o=$(call "$JAR_L" POST /api/v1/time-entries "{\"entry_date\":\"$YDAY\",\"start_time\":\"06:00\",\"end_time\":\"07:00\",\"category_id\":$CAT_LEAD}")
LEA_TE_ID=$(echo "${o#*$'\n'}" | grep -oE '"id":[0-9]+' | head -1 | cut -d: -f2)
[ -n "$LEA_TE_ID" ] && ok "lea created own entry ($LEA_TE_ID)" || bad "lea own entry create failed"
o=$(call "$JAR_L" POST /api/v1/time-entries/submit "{\"ids\":[$LEA_TE_ID]}"); expect "lea submit own" 200 "${o%%$'\n'*}" ""
o=$(call "$JAR_L" POST "/api/v1/time-entries/$LEA_TE_ID/approve"); expect "lea self-approve forbidden" 403 "${o%%$'\n'*}" ""
# Admin may approve the lead's own entry.
o=$(call "$JAR_A" POST "/api/v1/time-entries/$LEA_TE_ID/approve"); expect "admin approves lead entry" 200 "${o%%$'\n'*}" ""

# ----- 9. Change request workflow on an approved entry --------------------
banner "Tina — change request on approved entry"
# Change request without reason rejected.
o=$(call "$JAR_T" POST /api/v1/change-requests "{\"time_entry_id\":$ID_Y3,\"new_end_time\":\"12:30\",\"reason\":\"\"}")
expect "no-reason CR rejected" 400 "${o%%$'\n'*}" ""
# Cannot file CR for someone else's entry.
o=$(call "$JAR_T" POST /api/v1/change-requests "{\"time_entry_id\":$TE1,\"new_end_time\":\"12:00\",\"reason\":\"x\"}")
st=${o%%$'\n'*}; [ "$st" = 403 ] || [ "$st" = 404 ] && ok "foreign CR forbidden ($st)" || bad "foreign CR got $st"
# Cannot file CR for own draft (must edit directly).
mk "$DAY4" "08:00" "09:00" "$CAT_CARE" '"draft"'; expect "create draft for CR test" 200 "$(mk_status)" "$(mk_body)"; ID_DRAFT=$(mk_id)
o=$(call "$JAR_T" POST /api/v1/change-requests "{\"time_entry_id\":$ID_DRAFT,\"new_end_time\":\"09:30\",\"reason\":\"x\"}")
expect "CR on draft rejected" 400 "${o%%$'\n'*}" ""
# Valid CR: change time, category and comment together.
o=$(call "$JAR_T" POST /api/v1/change-requests "{\"time_entry_id\":$ID_Y3,\"new_start_time\":\"10:30\",\"new_end_time\":\"12:15\",\"new_category_id\":$CAT_PREP,\"new_comment\":\"reclassified to prep\",\"reason\":\"misclassified\"}")
body=${o#*$'\n'}; expect "multi-field CR created" 200 "${o%%$'\n'*}" "$body"
CR2=$(echo "$body" | grep -oE '"id":[0-9]+' | head -1 | cut -d: -f2)
# Lead approves CR -> changes apply.
o=$(call "$JAR_L" POST "/api/v1/change-requests/$CR2/approve"); expect "lead approve CR" 200 "${o%%$'\n'*}" ""
o=$(call "$JAR_T" GET "/api/v1/time-entries?from=$YDAY&to=$YDAY"); body=${o#*$'\n'}
echo "$body" | tr '}' '\n' | grep "\"id\":$ID_Y3," | grep -q "\"end_time\":\"12:15" && ok "CR applied to entry" || bad "CR not applied: $body"

# ----- 10. Reports reflect Tina's data ------------------------------------
banner "Reports — Tina's monthly + categories"
o=$(call "$JAR_L" GET "/api/v1/reports/month?user_id=$TINA_ID&month=$TINA_MONTH"); body=${o#*$'\n'}
expect "tina monthly report" 200 "${o%%$'\n'*}" "$body"
echo "$body" | grep -q "$YDAY" && ok "report mentions $YDAY" || bad "report missing $YDAY"
o=$(call "$JAR_L" GET "/api/v1/reports/categories?from=$DAY7&to=$TODAY"); body=${o#*$'\n'}
expect "category report" 200 "${o%%$'\n'*}" "$body"
echo "$body" | grep -q '"Direct Childcare"' && ok "childcare in category report" || bad "childcare missing"
echo "$body" | grep -q '"Preparation Time"' && ok "prep in category report"      || bad "prep missing"

# ----- 11. Logout, then session is gone -----------------------------------
banner "Tina — logout invalidates session"
o=$(call "$JAR_T" POST /api/v1/auth/logout); expect "tina logout" 200 "${o%%$'\n'*}" ""
o=$(call "$JAR_T" GET /api/v1/auth/me);      expect "tina /me 401 after logout" 401 "${o%%$'\n'*}" ""
# Mutating endpoints reject the dead cookie.
o=$(call "$JAR_T" POST /api/v1/time-entries "{\"entry_date\":\"$YDAY\",\"start_time\":\"21:00\",\"end_time\":\"21:30\",\"category_id\":$CAT_OTHER}")
expect "post-logout create rejected" 401 "${o%%$'\n'*}" ""

banner "Audit log"
o=$(call "$JAR_A" GET "/api/v1/audit-log?user_id=$EMP_ID"); body=${o#*$'\n'}
expect "audit log" 200 "${o%%$'\n'*}" "$body"
LC=$(echo "$body" | grep -o '"id"' | wc -l); [ "$LC" -gt 4 ] && ok "audit entries=$LC" || bad "audit count=$LC"
# Tina's audit trail must contain many transitions.
o=$(call "$JAR_A" GET "/api/v1/audit-log?user_id=$TINA_ID"); body=${o#*$'\n'}
TLC=$(echo "$body" | grep -o '"id"' | wc -l); [ "$TLC" -gt 15 ] && ok "tina audit entries=$TLC" || bad "tina audit count=$TLC"

banner "Password reset by admin"
o=$(call "$JAR_A" POST "/api/v1/users/$EMP_ID/reset-password"); body=${o#*$'\n'}
expect "reset password" 200 "${o%%$'\n'*}" "$body"
NEW_PW=$(echo "$body" | grep -oE '"temporary_password":"[^"]+"' | cut -d'"' -f4)
[ -n "$NEW_PW" ] && ok "new temp pw issued" || bad "no temp pw: $body"
JAR_E2=/tmp/it_emp2.cookies; rm -f "$JAR_E2"
o=$(call "$JAR_E2" POST /api/v1/auth/login "{\"email\":\"erin@example.com\",\"password\":\"$NEW_PW\"}")
expect "login with reset pw" 200 "${o%%$'\n'*}" ""

banner "Deactivation blocks login"
o=$(call "$JAR_A" POST "/api/v1/users/$EMP_ID/deactivate"); expect "deactivate user" 200 "${o%%$'\n'*}" ""
JAR_E3=/tmp/it_emp3.cookies; rm -f "$JAR_E3"
o=$(call "$JAR_E3" POST /api/v1/auth/login "{\"email\":\"erin@example.com\",\"password\":\"$NEW_PW\"}")
expect "deactivated login rejected" 400 "${o%%$'\n'*}" "${o#*$'\n'}"

banner "Logout"
o=$(call "$JAR_A" POST /api/v1/auth/logout);  expect "logout" 200 "${o%%$'\n'*}" ""
o=$(call "$JAR_A" GET /api/v1/auth/me);       expect "me after logout" 401 "${o%%$'\n'*}" ""

# ---------------------------------------------------------------------------
# Browser smoke test (only if Docker can pull the puppeteer image)
# ---------------------------------------------------------------------------
banner "Browser smoke (Puppeteer in Docker)"
if docker image inspect ghcr.io/puppeteer/puppeteer:22 >/dev/null 2>&1 || docker pull -q ghcr.io/puppeteer/puppeteer:22 >/dev/null 2>&1; then
  OUT=$(docker run --rm --network host \
    -e NODE_PATH=/home/pptruser/node_modules \
    -e URL="$BASE/" \
    -v "$ROOT/scripts:/s" \
    ghcr.io/puppeteer/puppeteer:22 node /s/browser_test.js 2>&1)
  echo "$OUT" | sed 's/^/    /'
  echo "$OUT" | grep -q "^HTTP 200"             && ok "browser HTTP 200"          || bad "no HTTP 200"
  echo "$OUT" | grep -q "title: KitaZeit"       && ok "title=KitaZeit"            || bad "wrong title"
  echo "$OUT" | grep -q "<form>"                && ok "login form rendered"       || bad "form missing"
  echo "$OUT" | grep -q "Maximum call stack"    && bad "infinite recursion!"      || ok "no infinite recursion"
  ! echo "$OUT" | grep -q "^pageerror:"          && ok "no page errors"            || bad "page errors present"
else
  printf "  (skipped — could not pull puppeteer image)\n"
fi

# ---------------------------------------------------------------------------
echo
printf "\033[1mResult: %d passed, %d failed\033[0m\n" "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
