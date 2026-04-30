#!/usr/bin/env bash
# KitaZeit automated integration test runner.
#
# Boots a clean app container on a private port (no Caddy, no public DNS),
# captures the auto-generated admin password from the logs, then runs:
#
#   1. API regression (curl + bash) against the local container
#   2. Headless browser smoke test (Puppeteer in Docker)
#
# Usage:  bash tests/run.sh
# Exit code is non-zero if any assertion fails.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

CONTAINER=kitazeit-it
PORT=${KITAZEIT_TEST_PORT:-3137}
BASE="http://127.0.0.1:$PORT"
DATA_DIR="$ROOT/.it-data"
IMG=zerf2-app:latest

PASS=0; FAIL=0
banner(){ printf "\n\033[1;36m== %s ==\033[0m\n" "$*"; }
ok()    { PASS=$((PASS+1)); printf "  \033[32m✓\033[0m %s\n" "$*"; }
bad()   { FAIL=$((FAIL+1)); printf "  \033[31m✗\033[0m %s\n" "$*"; }

cleanup(){
  docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
  sudo rm -rf "$DATA_DIR" 2>/dev/null || rm -rf "$DATA_DIR" 2>/dev/null || true
}
trap cleanup EXIT

banner "Build app image (cached layers reused)"
DOCKER_BUILDKIT=0 docker build -q -t "$IMG" "$ROOT" >/dev/null
ok "image built"

banner "Start ephemeral container on :$PORT"
cleanup
mkdir -p "$DATA_DIR"
sudo chown -R 10001:10001 "$DATA_DIR" 2>/dev/null || chown -R "$(id -u):$(id -g)" "$DATA_DIR" 2>/dev/null || true
docker run -d --name "$CONTAINER" \
  -p 127.0.0.1:$PORT:3000 \
  --user 10001:10001 \
  --read-only --tmpfs /tmp:size=16m \
  --cap-drop=ALL --security-opt=no-new-privileges:true \
  -e KITAZEIT_DATABASE_PATH=/app/data/kitazeit.db \
  -e KITAZEIT_SESSION_SECRET=integration-test-secret-do-not-use-in-prod-32-characters \
  -e KITAZEIT_ADMIN_EMAIL=admin@example.com \
  -e KITAZEIT_ORGANIZATION_NAME="Integration Test" \
  -e KITAZEIT_REGION=BW \
  -e KITAZEIT_DEV=1 \
  -e KITAZEIT_SECURE_COOKIES=false \
  -e KITAZEIT_ENFORCE_CSRF=false \
  -e KITAZEIT_ENFORCE_ORIGIN=false \
  -v "$DATA_DIR:/app/data" \
  "$IMG" >/dev/null
ok "container started ($CONTAINER)"

# Wait for readiness
for i in $(seq 1 40); do
  if curl -fsS "$BASE/" -o /dev/null 2>/dev/null; then ok "ready after ${i}x250ms"; break; fi
  sleep 0.25
  if [ "$i" = 40 ]; then bad "container did not become ready"; docker logs "$CONTAINER"; exit 1; fi
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

banner "Audit log"
o=$(call "$JAR_A" GET "/api/v1/audit-log?user_id=$EMP_ID"); body=${o#*$'\n'}
expect "audit log" 200 "${o%%$'\n'*}" "$body"
LC=$(echo "$body" | grep -o '"id"' | wc -l); [ "$LC" -gt 4 ] && ok "audit entries=$LC" || bad "audit count=$LC"

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
