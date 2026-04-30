#!/usr/bin/env bash
# Comprehensive API smoke test for KitaZeit.
# Resets the DB, picks the new admin password from logs, then exercises the API.
set -uo pipefail

BASE="${BASE:-https://REDACTED_DOMAIN}"
JAR_ADMIN=/tmp/kz_admin.cookies
JAR_EMP=/tmp/kz_emp.cookies
rm -f "$JAR_ADMIN" "$JAR_EMP"

PASS=0; FAIL=0
log()  { printf "\n\033[1;36m== %s ==\033[0m\n" "$*"; }
ok()   { PASS=$((PASS+1)); printf "  \033[32m✓\033[0m %s\n" "$*"; }
bad()  { FAIL=$((FAIL+1)); printf "  \033[31m✗\033[0m %s\n" "$*"; }

# Wrapper: jar method path [json]   -> echoes "<status>\n<body>"
req() {
  local jar=$1 method=$2 path=$3 data=${4:-}
  if [ -n "$data" ]; then
    curl -sS -b "$jar" -c "$jar" -o /tmp/kz_body -w "%{http_code}" \
      -H "Content-Type: application/json" -X "$method" --data "$data" "$BASE$path"
  else
    curl -sS -b "$jar" -c "$jar" -o /tmp/kz_body -w "%{http_code}" \
      -X "$method" "$BASE$path"
  fi
  echo
  cat /tmp/kz_body
}

expect() {
  local label=$1 want=$2 got=$3 body=$4
  if [ "$got" = "$want" ]; then ok "$label ($got)";
  else bad "$label expected=$want got=$got body=$body"; fi
}

log "Reset DB and capture initial admin password"
( cd "$(dirname "$0")/.." && docker compose down -t 2 >/dev/null 2>&1; sudo rm -rf data; docker compose up -d >/dev/null 2>&1 )
sleep 6
ADMIN_PW=$(cd "$(dirname "$0")/.." && docker compose logs app 2>&1 | grep -oE "Admin password: [A-Za-z0-9]+" | tail -1 | awk '{print $3}')
echo "  initial admin password: $ADMIN_PW"
[ -n "$ADMIN_PW" ] && ok "captured admin password" || { bad "no admin pw"; exit 1; }

log "Anonymous endpoints"
out=$(req "$JAR_ADMIN" GET /); st=${out%%$'\n'*}; body=${out#*$'\n'}; expect "GET /" 200 "$st" "$body"
out=$(req "$JAR_ADMIN" GET /api/v1/auth/me); st=${out%%$'\n'*}; body=${out#*$'\n'}; expect "/auth/me unauth" 401 "$st" "$body"
out=$(req "$JAR_ADMIN" GET /api/v1/users); st=${out%%$'\n'*}; body=${out#*$'\n'}; expect "/users unauth" 401 "$st" "$body"

log "Login admin"
out=$(req "$JAR_ADMIN" POST /api/v1/auth/login "{\"email\":\"admin@example.com\",\"password\":\"$ADMIN_PW\"}")
st=${out%%$'\n'*}; body=${out#*$'\n'}; expect "login admin" 200 "$st" "$body"

log "Force-change password (must_change_password)"
out=$(req "$JAR_ADMIN" PUT /api/v1/auth/password "{\"current_password\":\"$ADMIN_PW\",\"new_password\":\"AdminPass!234\"}")
st=${out%%$'\n'*}; body=${out#*$'\n'}; expect "change pw" 200 "$st" "$body"
out=$(req "$JAR_ADMIN" GET /api/v1/auth/me); st=${out%%$'\n'*}; body=${out#*$'\n'}; expect "/auth/me admin" 200 "$st" "$body"

log "Categories (default 6 expected)"
out=$(req "$JAR_ADMIN" GET /api/v1/categories); st=${out%%$'\n'*}; body=${out#*$'\n'}
expect "GET /categories" 200 "$st" "$body"
COUNT=$(echo "$body" | grep -o '"id"' | wc -l); [ "$COUNT" -ge 6 ] && ok "categories ≥6 ($COUNT)" || bad "categories count=$COUNT"
CAT_ID=$(echo "$body" | grep -oE '"id":[0-9]+' | head -1 | cut -d: -f2)

log "Create employee"
out=$(req "$JAR_ADMIN" POST /api/v1/users '{"email":"erin@example.com","first_name":"Erin","last_name":"Worker","role":"employee","weekly_hours":39,"annual_leave_days":30,"start_date":"2024-01-01"}')
st=${out%%$'\n'*}; body=${out#*$'\n'}; expect "create employee" 200 "$st" "$body"
EMP_ID=$(echo "$body" | grep -oE '"id":[0-9]+' | head -1 | cut -d: -f2)
EMP_PW=$(echo "$body" | grep -oE '"temporary_password":"[^"]+"' | cut -d'"' -f4)
echo "  employee id=$EMP_ID temp pw=$EMP_PW"

log "Create team_lead"
out=$(req "$JAR_ADMIN" POST /api/v1/users '{"email":"lead@example.com","first_name":"Lea","last_name":"Lead","role":"team_lead","weekly_hours":39,"annual_leave_days":30,"start_date":"2024-01-01"}')
st=${out%%$'\n'*}; body=${out#*$'\n'}; expect "create lead" 200 "$st" "$body"
LEAD_ID=$(echo "$body" | grep -oE '"id":[0-9]+' | head -1 | cut -d: -f2)
LEAD_PW=$(echo "$body" | grep -oE '"temporary_password":"[^"]+"' | cut -d'"' -f4)

log "Login employee + change pw"
out=$(req "$JAR_EMP" POST /api/v1/auth/login "{\"email\":\"erin@example.com\",\"password\":\"$EMP_PW\"}")
st=${out%%$'\n'*}; body=${out#*$'\n'}; expect "login emp" 200 "$st" "$body"
out=$(req "$JAR_EMP" PUT /api/v1/auth/password "{\"current_password\":\"$EMP_PW\",\"new_password\":\"EmpPass!234\"}")
st=${out%%$'\n'*}; expect "emp change pw" 200 "$st" ""

log "RBAC: employee cannot list users"
out=$(req "$JAR_EMP" GET /api/v1/users); st=${out%%$'\n'*}; body=${out#*$'\n'}; expect "emp /users forbidden" 403 "$st" "$body"

log "Time entries — create & validations"
TODAY=$(date -u +%F)
out=$(req "$JAR_EMP" POST /api/v1/time-entries "{\"entry_date\":\"$TODAY\",\"start_time\":\"08:00\",\"end_time\":\"12:00\",\"category_id\":$CAT_ID,\"comment\":\"morning\"}")
st=${out%%$'\n'*}; body=${out#*$'\n'}; expect "create entry 1" 200 "$st" "$body"
TE1=$(echo "$body" | grep -oE '"id":[0-9]+' | head -1 | cut -d: -f2)

out=$(req "$JAR_EMP" POST /api/v1/time-entries "{\"entry_date\":\"$TODAY\",\"start_time\":\"10:00\",\"end_time\":\"11:00\",\"category_id\":$CAT_ID}")
st=${out%%$'\n'*}; body=${out#*$'\n'}; expect "overlap rejected" 400 "$st" "$body"

out=$(req "$JAR_EMP" POST /api/v1/time-entries "{\"entry_date\":\"$TODAY\",\"start_time\":\"14:00\",\"end_time\":\"13:00\",\"category_id\":$CAT_ID}")
st=${out%%$'\n'*}; body=${out#*$'\n'}; expect "end<start rejected" 400 "$st" "$body"

FUTURE=$(date -u -d "+5 days" +%F)
out=$(req "$JAR_EMP" POST /api/v1/time-entries "{\"entry_date\":\"$FUTURE\",\"start_time\":\"08:00\",\"end_time\":\"09:00\",\"category_id\":$CAT_ID}")
st=${out%%$'\n'*}; body=${out#*$'\n'}; expect "future rejected" 400 "$st" "$body"

out=$(req "$JAR_EMP" POST /api/v1/time-entries "{\"entry_date\":\"$TODAY\",\"start_time\":\"13:00\",\"end_time\":\"15:00\",\"category_id\":$CAT_ID}")
st=${out%%$'\n'*}; body=${out#*$'\n'}; expect "create entry 2" 200 "$st" "$body"
TE2=$(echo "$body" | grep -oE '"id":[0-9]+' | head -1 | cut -d: -f2)

out=$(req "$JAR_EMP" GET "/api/v1/time-entries?from=$TODAY&to=$TODAY"); st=${out%%$'\n'*}; body=${out#*$'\n'}
expect "list own entries" 200 "$st" "$body"

log "Submit + approve workflow"
out=$(req "$JAR_EMP" POST /api/v1/time-entries/submit "{\"ids\":[$TE1,$TE2]}")
st=${out%%$'\n'*}; body=${out#*$'\n'}; expect "submit entries" 200 "$st" "$body"

# Login lead
JAR_LEAD=/tmp/kz_lead.cookies; rm -f "$JAR_LEAD"
out=$(req "$JAR_LEAD" POST /api/v1/auth/login "{\"email\":\"lead@example.com\",\"password\":\"$LEAD_PW\"}")
st=${out%%$'\n'*}; expect "lead login" 200 "$st" ""
out=$(req "$JAR_LEAD" PUT /api/v1/auth/password "{\"current_password\":\"$LEAD_PW\",\"new_password\":\"LeadPass!234\"}")
st=${out%%$'\n'*}; expect "lead change pw" 200 "$st" ""

out=$(req "$JAR_LEAD" POST "/api/v1/time-entries/$TE1/approve"); st=${out%%$'\n'*}; expect "approve TE1" 200 "$st" ""
out=$(req "$JAR_LEAD" POST "/api/v1/time-entries/$TE2/reject" '{"reason":"please clarify"}'); st=${out%%$'\n'*}; expect "reject TE2" 200 "$st" ""

# RBAC: employee cannot approve
out=$(req "$JAR_EMP" POST "/api/v1/time-entries/$TE1/approve"); st=${out%%$'\n'*}; expect "emp approve forbidden" 403 "$st" ""

log "Change request flow"
out=$(req "$JAR_EMP" POST /api/v1/change-requests "{\"time_entry_id\":$TE1,\"new_end_time\":\"12:30\",\"reason\":\"forgot 30 min\"}")
st=${out%%$'\n'*}; body=${out#*$'\n'}; expect "create change request" 200 "$st" "$body"
CR_ID=$(echo "$body" | grep -oE '"id":[0-9]+' | head -1 | cut -d: -f2)
out=$(req "$JAR_LEAD" POST "/api/v1/change-requests/$CR_ID/approve"); st=${out%%$'\n'*}; expect "approve change request" 200 "$st" ""

log "Absences"
V_FROM=$(date -u -d "+10 days" +%F); V_TO=$(date -u -d "+12 days" +%F)
out=$(req "$JAR_EMP" POST /api/v1/absences "{\"kind\":\"vacation\",\"start_date\":\"$V_FROM\",\"end_date\":\"$V_TO\"}")
st=${out%%$'\n'*}; body=${out#*$'\n'}; expect "request vacation" 200 "$st" "$body"
ABS_ID=$(echo "$body" | grep -oE '"id":[0-9]+' | head -1 | cut -d: -f2)
out=$(req "$JAR_EMP" POST /api/v1/absences "{\"kind\":\"sick\",\"start_date\":\"$TODAY\",\"end_date\":\"$TODAY\"}")
st=${out%%$'\n'*}; body=${out#*$'\n'}; expect "report sick (auto-approved)" 200 "$st" "$body"
echo "$body" | grep -q '"status":"approved"' && ok "sick is auto-approved" || bad "sick not auto-approved: $body"

out=$(req "$JAR_LEAD" POST "/api/v1/absences/$ABS_ID/approve"); st=${out%%$'\n'*}; expect "approve vacation" 200 "$st" ""

log "Vacation balance"
YEAR=$(date -u +%Y)
out=$(req "$JAR_EMP" GET "/api/v1/leave-balance/$EMP_ID?year=$YEAR"); st=${out%%$'\n'*}; body=${out#*$'\n'}
expect "leave balance" 200 "$st" "$body"
echo "  balance: $body"

log "Holidays + calendar + reports"
out=$(req "$JAR_ADMIN" GET "/api/v1/holidays?year=$YEAR"); st=${out%%$'\n'*}; body=${out#*$'\n'}
expect "holidays list" 200 "$st" "$body"
HC=$(echo "$body" | grep -o '"id"' | wc -l); [ "$HC" -ge 9 ] && ok "≥9 BW holidays ($HC)" || bad "holiday count=$HC"

MONTH=$(date -u +%Y-%m)
out=$(req "$JAR_LEAD" GET "/api/v1/absences/calendar?month=$MONTH"); st=${out%%$'\n'*}; expect "calendar" 200 "$st" ""
out=$(req "$JAR_LEAD" GET "/api/v1/reports/month?user_id=$EMP_ID&month=$MONTH"); st=${out%%$'\n'*}; expect "monthly report" 200 "$st" ""
out=$(req "$JAR_LEAD" GET "/api/v1/reports/team?month=$MONTH"); st=${out%%$'\n'*}; expect "team report" 200 "$st" ""
out=$(req "$JAR_LEAD" GET "/api/v1/reports/categories?from=$YEAR-01-01&to=$YEAR-12-31"); st=${out%%$'\n'*}; expect "category report" 200 "$st" ""
out=$(req "$JAR_LEAD" GET "/api/v1/reports/overtime?user_id=$EMP_ID&year=$YEAR"); st=${out%%$'\n'*}; expect "overtime report" 200 "$st" ""

CSV=$(curl -sS -b "$JAR_LEAD" -o /tmp/kz_csv -w "%{http_code} %{content_type}" "$BASE/api/v1/reports/month/csv?user_id=$EMP_ID&month=$MONTH")
echo "  csv: $CSV size=$(wc -c </tmp/kz_csv)"
echo "$CSV" | grep -q "^200" && ok "CSV export 200" || bad "CSV export $CSV"

log "Audit log (admin)"
out=$(req "$JAR_ADMIN" GET "/api/v1/audit-log?user_id=$EMP_ID"); st=${out%%$'\n'*}; body=${out#*$'\n'}
expect "audit log" 200 "$st" "$body"
LC=$(echo "$body" | grep -o '"id"' | wc -l); [ "$LC" -gt 0 ] && ok "audit entries=$LC" || bad "no audit entries"

log "Logout"
out=$(req "$JAR_ADMIN" POST /api/v1/auth/logout); st=${out%%$'\n'*}; expect "logout" 200 "$st" ""
out=$(req "$JAR_ADMIN" GET /api/v1/auth/me); st=${out%%$'\n'*}; expect "me after logout" 401 "$st" ""

echo
printf "\n\033[1mResult: %d passed, %d failed\033[0m\n" "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
