//! The flextime adjustment ledger that replaced the old, editable
//! `users.overtime_start_balance_min` setting (migration 043, which also drops
//! that column).
//!
//! The invariant under test throughout: a balance only ever changes on the
//! date a booking is dated, and only an admin can make one.

use chrono::Datelike;
use reqwest::StatusCode;
use serde_json::json;

use crate::common::{TestApp, TestClient};
use crate::helpers::{
    admin_login, bootstrap_team, id, login_change_pw, next_monday, reference_date,
    set_flextime_opening_balance, temp_pw,
};

/// Reads the day rows of a flextime report for `[from, to]`.
async fn flextime_days(
    client: &TestClient,
    user_id: i64,
    from: &str,
    to: &str,
) -> Vec<serde_json::Value> {
    let (st, body) = client
        .get(&format!(
            "/api/v1/reports/flextime?user_id={user_id}&from={from}&to={to}"
        ))
        .await;
    assert_eq!(st, StatusCode::OK, "flextime report: {body}");
    body["days"].as_array().cloned().unwrap_or_default()
}

/// Creating a user with a carry-in balance books it as the account's opening
/// ledger entry, dated on the start date — the balance is 0 the day before and
/// the carried value from the start date on. This is the exact behaviour the
/// old `overtime_start_balance_min` column produced, now stored as data.
#[tokio::test]
async fn opening_balance_is_booked_on_the_start_date() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let start_day = reference_date();
    let day_before = start_day - chrono::Duration::days(1);
    let start_iso = start_day.format("%Y-%m-%d").to_string();
    let day_before_iso = day_before.format("%Y-%m-%d").to_string();

    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "opening@example.com",
                "first_name": "Olive", "last_name": "Opening",
                "role": "employee", "weekly_hours": 39,
                "start_date": start_iso,
                "approver_ids": [1],
                "flextime_opening_balance_min": 600
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create user with carry-in: {body}");
    let user_id = id(&body);

    let days = flextime_days(&admin, user_id, &day_before_iso, &start_iso).await;
    assert_eq!(days.len(), 2, "two day rows expected: {days:?}");
    assert_eq!(
        days[0]["cumulative_min"], 0,
        "no balance exists before the start date"
    );
    assert_eq!(days[0]["adjustment_min"], 0);
    assert_eq!(
        days[1]["adjustment_min"], 600,
        "the carry-in lands on the start date"
    );

    // The account view reports the same booking, flagged as the opening one.
    let (st, account) = admin
        .get(&format!("/api/v1/users/{user_id}/flextime-account"))
        .await;
    assert_eq!(st, StatusCode::OK, "flextime account: {account}");
    assert_eq!(account["has_flextime_account"], true);
    let adjustments = account["adjustments"].as_array().unwrap();
    assert_eq!(adjustments.len(), 1, "one opening booking: {account}");
    assert_eq!(adjustments[0]["kind"], "opening_balance");
    assert_eq!(adjustments[0]["minutes"], 600);
    assert_eq!(adjustments[0]["effective_date"], start_iso);

    app.cleanup().await;
}

/// A correction only moves the balance from its effective date onwards; every
/// earlier day keeps the number it already had. This is the whole point of the
/// change: history can no longer be rewritten retroactively.
#[tokio::test]
async fn a_correction_moves_the_balance_only_from_its_date_onwards() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    let start_day = reference_date() - chrono::Duration::days(10);
    let correction_day = reference_date() - chrono::Duration::days(4);
    let start_iso = start_day.format("%Y-%m-%d").to_string();
    let correction_iso = correction_day.format("%Y-%m-%d").to_string();
    let day_before_correction_iso = (correction_day - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();

    // weekly_hours 0 so no daily target muddies the numbers: every balance
    // change in this test comes from a booking.
    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "correct@example.com",
                "first_name": "Cora", "last_name": "Correction",
                "role": "employee", "weekly_hours": 0,
                "start_date": start_iso,
                "approver_ids": [1],
                "flextime_opening_balance_min": 120
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create user: {body}");
    let user_id = id(&body);

    let (st, created) = admin
        .post(
            &format!("/api/v1/users/{user_id}/flextime-adjustments"),
            &json!({"effective_date": correction_iso, "minutes": -60, "reason": "  Overtime payout  "}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "book correction: {created}");
    assert_eq!(created["kind"], "correction");
    assert_eq!(
        created["reason"], "Overtime payout",
        "the note is trimmed before storing"
    );
    let adjustment_id = id(&created);

    let days = flextime_days(&admin, user_id, &start_iso, &correction_iso).await;
    let day_before = days
        .iter()
        .find(|d| d["date"] == day_before_correction_iso.as_str())
        .expect("day before the correction");
    assert_eq!(
        day_before["cumulative_min"], 120,
        "the balance before the correction is untouched"
    );
    let correction_row = days
        .iter()
        .find(|d| d["date"] == correction_iso.as_str())
        .expect("correction day");
    assert_eq!(correction_row["adjustment_min"], -60);
    assert_eq!(correction_row["cumulative_min"], 60);

    // Cancelling the booking puts the balance back where it was — by writing
    // the opposite entry, not by removing anything. The day's net adjustment
    // is zero again while both rows stay on the record.
    let (st, reversal) = admin
        .post(
            &format!("/api/v1/flextime-adjustments/{adjustment_id}/reverse"),
            &json!({}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "cancel the correction: {reversal}");
    assert_eq!(reversal["minutes"], 60, "the opposite amount");
    assert_eq!(reversal["effective_date"], correction_iso.as_str());
    assert_eq!(reversal["reverses_id"], adjustment_id);

    let days = flextime_days(&admin, user_id, &correction_iso, &correction_iso).await;
    assert_eq!(days[0]["adjustment_min"], 0);
    assert_eq!(days[0]["cumulative_min"], 120);

    let (st, account) = admin
        .get(&format!("/api/v1/users/{user_id}/flextime-account"))
        .await;
    assert_eq!(st, StatusCode::OK, "flextime account: {account}");
    let adjustments = account["adjustments"].as_array().unwrap();
    assert_eq!(
        adjustments.len(),
        3,
        "nothing is removed: carry-in, mistake, cancellation: {account}"
    );
    assert_eq!(
        adjustments[1]["reversed"], true,
        "the cancelled entry is marked as such"
    );

    // A cancellation cannot itself be cancelled, and an entry cannot be
    // cancelled twice — either would swing the balance instead of restoring it.
    let (st, body) = admin
        .post(
            &format!("/api/v1/flextime-adjustments/{adjustment_id}/reverse"),
            &json!({}),
        )
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "no double cancellation: {body}");
    let reversal_id = id(&reversal);
    let (st, body) = admin
        .post(
            &format!("/api/v1/flextime-adjustments/{reversal_id}/reverse"),
            &json!({}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::BAD_REQUEST,
        "a cancellation is not itself cancellable: {body}"
    );

    app.cleanup().await;
}

/// Only admins may book. Employees and team leads may read the account (a
/// balance nobody can explain is worse than one they can), but not change it.
#[tokio::test]
async fn only_admins_may_book_adjustments() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_lead_id, lead_pw, emp_id, emp_pw, _monday, _cat) =
        bootstrap_team(&app, &admin, false).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;
    let lead = login_change_pw(&app, "lead-r@example.com", &lead_pw).await;
    let today_iso = reference_date().format("%Y-%m-%d").to_string();
    let payload = json!({"effective_date": today_iso, "minutes": 60});

    let (st, _) = emp
        .post(
            &format!("/api/v1/users/{emp_id}/flextime-adjustments"),
            &payload,
        )
        .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "employee cannot book for self");

    let (st, _) = lead
        .post(
            &format!("/api/v1/users/{emp_id}/flextime-adjustments"),
            &payload,
        )
        .await;
    assert_eq!(
        st,
        StatusCode::FORBIDDEN,
        "team lead cannot book for a report"
    );

    // Reading is allowed for both.
    let (st, account) = emp
        .get(&format!("/api/v1/users/{emp_id}/flextime-account"))
        .await;
    assert_eq!(st, StatusCode::OK, "employee reads own account: {account}");
    let (st, _) = lead
        .get(&format!("/api/v1/users/{emp_id}/flextime-account"))
        .await;
    assert_eq!(st, StatusCode::OK, "lead reads a report's account");

    // Cancelling is admin-only too.
    let (st, created) = admin
        .post(
            &format!("/api/v1/users/{emp_id}/flextime-adjustments"),
            &payload,
        )
        .await;
    assert_eq!(st, StatusCode::OK, "admin books: {created}");
    let (st, _) = emp
        .post(
            &format!("/api/v1/flextime-adjustments/{}/reverse", id(&created)),
            &json!({}),
        )
        .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "employee cannot cancel a booking");

    app.cleanup().await;
}

/// Dates outside the account's lifetime, zero, and oversized values are
/// rejected rather than silently clamped — an admin must never be shown a
/// date or amount that was not what got stored.
#[tokio::test]
async fn adjustment_input_is_validated() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_lead_id, _lead_pw, emp_id, _emp_pw, _monday, _cat) =
        bootstrap_team(&app, &admin, false).await;
    let today = reference_date();
    let today_iso = today.format("%Y-%m-%d").to_string();
    let path = format!("/api/v1/users/{emp_id}/flextime-adjustments");

    let (st, body) = admin
        .post(&path, &json!({"effective_date": today_iso, "minutes": 0}))
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "zero rejected: {body}");

    let (st, body) = admin
        .post(
            &path,
            &json!({"effective_date": today_iso, "minutes": 525_601}),
        )
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "oversized rejected: {body}");

    // A date still ahead is accepted on purpose: an overtime payout agreed for
    // month end is recorded when it is agreed and takes effect on its day.
    let tomorrow_iso = (today + chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    let (st, body) = admin
        .post(
            &path,
            &json!({"effective_date": tomorrow_iso, "minutes": 60}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "a future date is allowed: {body}");

    // bootstrap_team starts the employee on 2024-01-01.
    let (st, body) = admin
        .post(
            &path,
            &json!({"effective_date": "2023-12-31", "minutes": 60}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::BAD_REQUEST,
        "date before the start date rejected: {body}"
    );

    let (st, body) = admin
        .post(
            &path,
            &json!({"effective_date": today_iso, "minutes": 60, "reason": "x".repeat(501)}),
        )
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "over-long note rejected: {body}");

    app.cleanup().await;
}

/// Assistants have no flextime account at all, so there is no balance to
/// adjust — the API says so instead of writing a row nothing would ever read.
#[tokio::test]
async fn assistants_have_no_flextime_account_to_adjust() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, _lead_pw, _emp_id, _emp_pw, _monday, _cat) =
        bootstrap_team(&app, &admin, false).await;

    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "assist-flex@example.com",
                "first_name": "Aida", "last_name": "Assist",
                "role": "assistant", "weekly_hours": 0,
                "start_date": "2024-01-01",
                "approver_ids": [lead_id]
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create assistant: {body}");
    let assistant_id = id(&body);

    let (st, account) = admin
        .get(&format!("/api/v1/users/{assistant_id}/flextime-account"))
        .await;
    assert_eq!(st, StatusCode::OK, "account readable: {account}");
    assert_eq!(account["has_flextime_account"], false);
    assert!(account["balance_min"].is_null());

    let today_iso = reference_date().format("%Y-%m-%d").to_string();
    let (st, body) = admin
        .post(
            &format!("/api/v1/users/{assistant_id}/flextime-adjustments"),
            &json!({"effective_date": today_iso, "minutes": 60}),
        )
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "booking rejected: {body}");

    app.cleanup().await;
}

/// The monthly overtime rows the dashboard reads must report a booking on its
/// own line and carry it into the running balance, so a jump in the balance is
/// always traceable to something visible.
#[tokio::test]
async fn overtime_rows_report_adjustments_separately() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let today = reference_date();
    // First day of the current month, so the booking and the report share a
    // month regardless of when the suite runs.
    let month_start = today.with_day(1).expect("first of month");
    let month_label = month_start.format("%Y-%m").to_string();
    let start_iso = month_start.format("%Y-%m-%d").to_string();
    let today_iso = today.format("%Y-%m-%d").to_string();

    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "rows@example.com",
                "first_name": "Rita", "last_name": "Rows",
                "role": "employee", "weekly_hours": 0,
                "start_date": start_iso,
                "approver_ids": [1]
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create user: {body}");
    let user_id = id(&body);

    let (st, body) = admin
        .post(
            &format!("/api/v1/users/{user_id}/flextime-adjustments"),
            &json!({"effective_date": today_iso, "minutes": 180}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "book correction: {body}");

    let (st, body) = admin
        .get(&format!(
            "/api/v1/reports/overtime?user_id={user_id}&year={}",
            month_start.format("%Y")
        ))
        .await;
    assert_eq!(st, StatusCode::OK, "overtime rows: {body}");
    let row = body["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["month"] == month_label.as_str())
        .unwrap_or_else(|| panic!("row for {month_label}: {body}"));
    assert_eq!(row["adjustment_min"], 180, "booking reported on its own");
    assert_eq!(
        row["diff_min"], 0,
        "worked-vs-target stays free of admin bookings"
    );
    assert_eq!(row["cumulative_min"], 180, "the balance carries it");

    app.cleanup().await;
}

/// Bookings are not gated on week approval: an admin correcting a balance
/// today must see it immediately, even though the flextime cutoff still sits
/// at the end of the last fully approved week.
#[tokio::test]
async fn a_booking_after_the_cutoff_counts_immediately() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let today = reference_date();
    let start_iso = (today - chrono::Duration::days(3))
        .format("%Y-%m-%d")
        .to_string();
    let today_iso = today.format("%Y-%m-%d").to_string();

    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "cutoff@example.com",
                "first_name": "Cleo", "last_name": "Cutoff",
                "role": "employee", "weekly_hours": 0,
                "start_date": start_iso,
                "approver_ids": [1]
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create user: {body}");
    let user_id = id(&body);

    let (st, body) = admin
        .post(
            &format!("/api/v1/users/{user_id}/flextime-adjustments"),
            &json!({"effective_date": today_iso, "minutes": 240}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "book correction: {body}");

    let (st, account) = admin
        .get(&format!("/api/v1/users/{user_id}/flextime-account"))
        .await;
    assert_eq!(st, StatusCode::OK, "flextime account: {account}");
    assert_eq!(
        account["balance_min"], 240,
        "the balance shows the booking right away: {account}"
    );

    app.cleanup().await;
}

/// A user created with time tracking switched off has no ledger to book into,
/// so a carry-in balance sent along with them is dropped rather than stored
/// where nothing would ever read it.
#[tokio::test]
async fn a_pure_admin_gets_no_opening_balance() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let start_iso = reference_date().format("%Y-%m-%d").to_string();

    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "pure@example.com",
                "first_name": "Pat", "last_name": "Pureadmin",
                "role": "admin", "weekly_hours": 39,
                "start_date": start_iso,
                "tracks_time": false,
                "flextime_opening_balance_min": 300
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create pure admin: {body}");
    let user_id = id(&body);
    // Keep the generated password from tripping the unused-variable lint and
    // document that creation really did go through the normal path.
    assert!(!temp_pw(&body).is_empty());

    let (st, account) = admin
        .get(&format!("/api/v1/users/{user_id}/flextime-account"))
        .await;
    assert_eq!(st, StatusCode::OK, "account readable: {account}");
    assert_eq!(account["has_flextime_account"], false);
    assert!(
        account["adjustments"].as_array().unwrap().is_empty(),
        "no booking for an account without a ledger: {account}"
    );

    app.cleanup().await;
}

/// Cross-checks the two independent balance pipelines against each other over
/// a span of months: the monthly overtime rows (used by the dashboard and team
/// report) and the per-day flextime ledger (used by the chart, PDF and CSV).
///
/// They accumulate adjustments completely differently — one buckets by month
/// and walks every month since the contract start, the other seeds from a
/// month-end balance and then replays day by day — so an off-by-one in either
/// shows up here as a disagreement.
#[tokio::test]
async fn month_rows_and_the_day_ledger_agree_across_months() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let today = reference_date();
    // Far enough back that the carry-in, the correction and "today" land in
    // three different months, whichever day of the year the suite runs on.
    let start_day = today - chrono::Duration::days(70);
    let correction_day = today - chrono::Duration::days(35);
    let start_iso = start_day.format("%Y-%m-%d").to_string();
    let correction_iso = correction_day.format("%Y-%m-%d").to_string();

    // weekly_hours 0: no daily target, so every movement below comes from a
    // booking and the arithmetic is exact.
    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "span@example.com",
                "first_name": "Sven", "last_name": "Span",
                "role": "employee", "weekly_hours": 0,
                "start_date": start_iso,
                "approver_ids": [1],
                "flextime_opening_balance_min": 600
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create user: {body}");
    let user_id = id(&body);

    let (st, body) = admin
        .post(
            &format!("/api/v1/users/{user_id}/flextime-adjustments"),
            &json!({"effective_date": correction_iso, "minutes": -120}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "book correction: {body}");

    // Monthly rows: the current month's running balance holds both bookings,
    // including the ones made in earlier months (and, when the contract start
    // falls in the previous calendar year, in an earlier year).
    let (st, body) = admin
        .get(&format!(
            "/api/v1/reports/overtime?user_id={user_id}&year={}",
            today.format("%Y")
        ))
        .await;
    assert_eq!(st, StatusCode::OK, "overtime rows: {body}");
    let current_month = today.format("%Y-%m").to_string();
    let row = body["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["month"] == current_month.as_str())
        .unwrap_or_else(|| panic!("row for {current_month}: {body}"));
    assert_eq!(row["cumulative_min"], 480, "600 carried in, 120 booked out");

    // Day ledger, seeded from months of history: same number.
    let today_iso = today.format("%Y-%m-%d").to_string();
    let days = flextime_days(&admin, user_id, &today_iso, &today_iso).await;
    assert_eq!(
        days[0]["cumulative_min"], 480,
        "the day ledger must agree with the monthly rows"
    );

    // And the step itself sits exactly on the correction's date.
    let day_before_iso = (correction_day - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    let days = flextime_days(&admin, user_id, &day_before_iso, &correction_iso).await;
    assert_eq!(days[0]["cumulative_min"], 600, "untouched before the booking");
    assert_eq!(days[1]["adjustment_min"], -120);
    assert_eq!(days[1]["cumulative_min"], 480);

    app.cleanup().await;
}

/// Moving a contract start date forward must relocate an earlier booking, not
/// silently drop it out of every balance. The ledger only exists from the
/// start date on, so the booking is reported on that first day instead.
#[tokio::test]
async fn moving_the_start_date_forward_keeps_an_earlier_booking() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let today = reference_date();
    let original_start = today - chrono::Duration::days(60);
    let new_start = today - chrono::Duration::days(30);
    let new_start_iso = new_start.format("%Y-%m-%d").to_string();

    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "moved@example.com",
                "first_name": "Mira", "last_name": "Moved",
                "role": "employee", "weekly_hours": 0,
                "start_date": original_start.format("%Y-%m-%d").to_string(),
                "approver_ids": [1],
                "flextime_opening_balance_min": 300
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create user: {body}");
    let user_id = id(&body);

    let (st, body) = admin
        .put(
            &format!("/api/v1/users/{user_id}"),
            &json!({"start_date": new_start_iso, "approver_ids": [1]}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "move the start date: {body}");

    // The booking still carries its original date in the ledger listing…
    let (st, account) = admin
        .get(&format!("/api/v1/users/{user_id}/flextime-account"))
        .await;
    assert_eq!(st, StatusCode::OK, "flextime account: {account}");
    assert_eq!(
        account["adjustments"][0]["effective_date"],
        original_start.format("%Y-%m-%d").to_string().as_str(),
        "the booking keeps the date it was made for: {account}"
    );
    assert_eq!(account["balance_min"], 300, "and still counts: {account}");

    // …but takes effect on the new first day of the ledger.
    let day_before_iso = (new_start - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    let days = flextime_days(&admin, user_id, &day_before_iso, &new_start_iso).await;
    assert_eq!(days[0]["cumulative_min"], 0, "nothing before the contract");
    assert_eq!(days[1]["adjustment_min"], 300);
    assert_eq!(days[1]["cumulative_min"], 300);

    app.cleanup().await;
}

/// The upgrade path itself: migration 043 has to move every existing carry-in
/// balance into the ledger without losing one, and stay safe to re-run.
///
/// The migration always runs before any user exists in a test database, so its
/// backfill branch would otherwise never be exercised at all. This test rebuilds
/// a genuine pre-043 database — the old column back on `users`, no ledger table
/// — and replays the shipped SQL verbatim, reading the real file rather than a
/// copy so the test cannot drift from what production runs.
#[tokio::test]
async fn migration_043_backfills_existing_carry_in_balances() {
    const MIGRATION_043: &str = include_str!("../../migrations/043_flextime_adjustments.sql");

    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (lead_id, _lead_pw, emp_id, _emp_pw, _monday, _cat) =
        bootstrap_team(&app, &admin, false).await;

    // Recreate a pre-043 database: the old column back in place and no ledger
    // table at all. Dropping the table rather than emptying it matters — it
    // puts the constraint back in the state the real upgrade starts from.
    sqlx::query("DROP TABLE flextime_adjustments")
        .execute(&app.state.pool)
        .await
        .expect("drop the ledger table");
    sqlx::query(
        "ALTER TABLE users ADD COLUMN overtime_start_balance_min BIGINT NOT NULL DEFAULT 0",
    )
    .execute(&app.state.pool)
    .await
    .expect("restore the legacy column");
    // A plain balance, and one far outside the range the API would accept —
    // only reachable by editing the database by hand, but a migration that
    // aborts on it would leave the application unable to start at all.
    sqlx::query("UPDATE users SET overtime_start_balance_min = 480 WHERE id = $1")
        .bind(emp_id)
        .execute(&app.state.pool)
        .await
        .expect("write the legacy column");
    sqlx::query("UPDATE users SET overtime_start_balance_min = 9999999 WHERE id = $1")
        .bind(lead_id)
        .execute(&app.state.pool)
        .await
        .expect("write an out-of-range legacy value");

    sqlx::raw_sql(MIGRATION_043)
        .execute(&app.state.pool)
        .await
        .expect("replay migration 043");

    let rows: Vec<(i64, chrono::NaiveDate, i64, String, Option<i64>)> = sqlx::query_as(
        "SELECT user_id, effective_date, minutes, kind, created_by \
         FROM flextime_adjustments ORDER BY minutes",
    )
    .fetch_all(&app.state.pool)
    .await
    .expect("read the backfilled ledger");

    // Two rows, for the two users who carried a balance. The admin carries 0,
    // which holds no information, so they get no row.
    assert_eq!(rows.len(), 2, "only non-zero balances are migrated: {rows:?}");
    for (_, effective_date, _, kind, created_by) in &rows {
        assert_eq!(kind, "opening_balance");
        assert_eq!(
            *created_by, None,
            "migrated rows are attributed to the system, not to whoever upgraded"
        );
        assert_eq!(
            effective_date.format("%Y-%m-%d").to_string(),
            "2024-01-01",
            "dated on the user's start date, where the old code injected it"
        );
    }
    assert_eq!(rows[0].0, emp_id);
    assert_eq!(rows[0].2, 480, "the exact carried value, not a rounded one");
    assert_eq!(rows[1].0, lead_id);
    assert_eq!(
        rows[1].2, 9_999_999,
        "an out-of-range legacy value is carried across, not clamped or dropped"
    );

    // The balance the app reports must match what the old column meant.
    let (st, account) = admin
        .get(&format!("/api/v1/users/{emp_id}/flextime-account"))
        .await;
    assert_eq!(st, StatusCode::OK, "flextime account: {account}");
    assert_eq!(account["adjustments"].as_array().unwrap().len(), 1);
    assert_eq!(account["adjustments"][0]["minutes"], 480);

    // Re-running must not duplicate anything: an interrupted upgrade or a
    // re-applied migration would otherwise double every carried balance.
    sqlx::raw_sql(MIGRATION_043)
        .execute(&app.state.pool)
        .await
        .expect("replay migration 043 a second time");
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM flextime_adjustments")
        .fetch_one(&app.state.pool)
        .await
        .expect("count ledger rows");
    assert_eq!(count, 2, "the migration is idempotent");

    // Exempting the migrated rows must not weaken the rule going forward.
    let today_iso = reference_date().format("%Y-%m-%d").to_string();
    let (st, body) = admin
        .post(
            &format!("/api/v1/users/{emp_id}/flextime-adjustments"),
            &json!({"effective_date": today_iso, "minutes": 9_999_999}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::BAD_REQUEST,
        "new bookings stay bounded: {body}"
    );

    // The source column is gone: every value it held is now a ledger row, and
    // leaving a second writable copy of the same fact behind is exactly the
    // ambiguity this migration removes.
    let legacy_column_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM information_schema.columns \
         WHERE table_schema = current_schema() AND table_name = 'users' \
         AND column_name = 'overtime_start_balance_min')",
    )
    .fetch_one(&app.state.pool)
    .await
    .expect("look up the legacy column");
    assert!(
        !legacy_column_exists,
        "the migration must drop the column it read from"
    );

    app.cleanup().await;
}

/// A debit booked after the flextime cutoff has to protect the balance floor
/// immediately. The approved-hours ledger stops at the cutoff, so a booking
/// made today is invisible to it — if the floor check ignored that too, an
/// employee could spend hours an admin had already taken away.
#[tokio::test]
async fn a_debit_after_the_cutoff_guards_the_balance_floor() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let (_lead_id, _lead_pw, emp_id, emp_pw, _monday, _cat) =
        bootstrap_team(&app, &admin, false).await;
    let emp = login_change_pw(&app, "emp-r@example.com", &emp_pw).await;

    // 39h over 5 days = 468 minutes of target per day, which is what one day
    // of flextime reduction costs. Three days' worth of balance to start.
    const DAILY_TARGET_MIN: i64 = 468;
    set_flextime_opening_balance(&app.state.pool, emp_id, DAILY_TARGET_MIN * 3)
        .await
        .expect("seed flextime balance");

    let first_monday = next_monday(7).format("%Y-%m-%d").to_string();
    let (st, body) = emp
        .post(
            "/api/v1/absences",
            &json!({"kind":"flextime_reduction","start_date":first_monday,"end_date":first_monday}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::OK,
        "one day fits inside a three-day balance: {body}"
    );

    // Take almost all of it away, dated today — well after the cutoff, which
    // for this employee still sits before their start date.
    let today_iso = reference_date().format("%Y-%m-%d").to_string();
    let (st, body) = admin
        .post(
            &format!("/api/v1/users/{emp_id}/flextime-adjustments"),
            &json!({
                "effective_date": today_iso,
                "minutes": -(DAILY_TARGET_MIN * 2 + 100),
                "reason": "Overtime paid out"
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "book the debit: {body}");

    let second_monday = next_monday(14).format("%Y-%m-%d").to_string();
    let (st, body) = emp
        .post(
            "/api/v1/absences",
            &json!({"kind":"flextime_reduction","start_date":second_monday,"end_date":second_monday}),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::BAD_REQUEST,
        "the hours were paid out, so they cannot be taken as time off too: {body}"
    );

    app.cleanup().await;
}

/// A booking dated ahead of today is recorded now and takes effect on its day.
/// Today's balance must not move yet — otherwise agreeing a payout for month
/// end would silently spend the hours the moment it is written down.
#[tokio::test]
async fn a_future_booking_only_takes_effect_on_its_own_date() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;
    let today = reference_date();
    let start_iso = (today - chrono::Duration::days(10))
        .format("%Y-%m-%d")
        .to_string();
    let payout_day = today + chrono::Duration::days(20);
    let payout_iso = payout_day.format("%Y-%m-%d").to_string();
    let today_iso = today.format("%Y-%m-%d").to_string();

    // weekly_hours 0 keeps every movement attributable to a booking.
    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "future@example.com",
                "first_name": "Fritz", "last_name": "Future",
                "role": "employee", "weekly_hours": 0,
                "start_date": start_iso,
                "approver_ids": [1],
                "flextime_opening_balance_min": 600
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create user: {body}");
    let user_id = id(&body);

    let (st, body) = admin
        .post(
            &format!("/api/v1/users/{user_id}/flextime-adjustments"),
            &json!({
                "effective_date": payout_iso,
                "minutes": -300,
                "reason": "Payout agreed for month end"
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "book a future payout: {body}");

    // Today's balance is untouched, and the entry is on the record already.
    let (st, account) = admin
        .get(&format!("/api/v1/users/{user_id}/flextime-account"))
        .await;
    assert_eq!(st, StatusCode::OK, "flextime account: {account}");
    assert_eq!(
        account["balance_min"], 600,
        "a booking dated ahead does not move today's balance: {account}"
    );
    assert_eq!(account["adjustments"].as_array().unwrap().len(), 2);

    let days = flextime_days(&admin, user_id, &today_iso, &today_iso).await;
    assert_eq!(days[0]["cumulative_min"], 600);

    // Reading the ledger up to the payout day shows it applied.
    let days = flextime_days(&admin, user_id, &payout_iso, &payout_iso).await;
    assert_eq!(days[0]["adjustment_min"], -300);
    assert_eq!(days[0]["cumulative_min"], 300);

    // The monthly rows must agree with today's balance, not with the month the
    // payout happens to fall in. Grouping bookings by month without capping
    // them at today let a payout dated later in the *current* month leak into
    // the balance the dashboard shows, while the account dialog still showed
    // the old one — two views, two numbers for the same person.
    //
    // Both assertions hold whichever month the payout lands in, but they only
    // *catch* the leak when it lands in the current one. CI pins
    // TEST_REFERENCE_DATE to 2030-01-07, so +20 days stays in January and the
    // regression is caught deterministically there.
    let (st, body) = admin
        .get(&format!(
            "/api/v1/reports/overtime?user_id={user_id}&year={}",
            today.format("%Y")
        ))
        .await;
    assert_eq!(st, StatusCode::OK, "overtime rows: {body}");
    let current_month = today.format("%Y-%m").to_string();
    let row = body["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["month"] == current_month.as_str())
        .unwrap_or_else(|| panic!("row for {current_month}: {body}"));
    assert_eq!(
        row["cumulative_min"], 600,
        "a booking that has not taken effect must not move the running balance: {body}"
    );
    assert_eq!(
        row["adjustment_min"], 0,
        "and must not be reported as this month's adjustment either: {body}"
    );

    app.cleanup().await;
}

/// A booking (or its reversal) dated in an already-archived month must
/// re-queue that month's timesheet export — otherwise a PDF already uploaded
/// to Nextcloud keeps showing the old closing balance forever, even though
/// the app itself now reports a different one. No other mutation in the app
/// changes a past month's rendered balance without going through
/// `requeue_export_for_dates`/`requeue_export_for_absence_period`; bookings
/// used to be the one exception.
#[tokio::test]
async fn a_booking_in_a_past_month_requeues_its_export() {
    let app = TestApp::spawn().await;
    let admin = admin_login(&app).await;

    // Last day of the previous month: guaranteed to be in an already-past
    // month regardless of what day of the month the suite runs on, and the
    // walk from this single day through today never reaches a second past
    // month (only the current one, which is deliberately excluded from
    // requeuing because it has not been archived yet).
    let today = reference_date();
    let first_of_this_month = today.with_day(1).expect("valid date");
    let booking_day = first_of_this_month - chrono::Duration::days(1);
    let booking_period = booking_day.format("%Y-%m").to_string();
    let booking_iso = booking_day.format("%Y-%m-%d").to_string();
    let start_iso = (booking_day - chrono::Duration::days(30))
        .format("%Y-%m-%d")
        .to_string();

    let (st, body) = admin
        .post(
            "/api/v1/users",
            &json!({
                "email": "requeue@example.com",
                "first_name": "Rex", "last_name": "Requeue",
                "role": "employee", "weekly_hours": 39,
                "start_date": start_iso,
                "approver_ids": [1]
            }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "create user: {body}");
    let user_id = id(&body);

    // Feature disabled (the default): booking must not queue anything.
    let (st, body) = admin
        .post(
            &format!("/api/v1/users/{user_id}/flextime-adjustments"),
            &json!({"effective_date": booking_iso, "minutes": 120}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "book while upload disabled: {body}");
    let pending = app.state.db.export_queue.list_pending().await.unwrap();
    assert!(
        pending.is_empty(),
        "no requeue while report upload is disabled"
    );

    app.state
        .db
        .settings
        .save_setting(zerf::services::settings::REPORT_UPLOAD_ENABLED_KEY, "true")
        .await
        .expect("enable report upload");

    let (st, body) = admin
        .post(
            &format!("/api/v1/users/{user_id}/flextime-adjustments"),
            &json!({"effective_date": booking_iso, "minutes": -50, "reason": "correction"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "book while upload enabled: {body}");
    let adjustment_id = id(&body);

    let pending = app.state.db.export_queue.list_pending().await.unwrap();
    let pending_pairs: Vec<(i64, String)> = pending
        .iter()
        .map(|entry| (entry.user_id, entry.period.clone()))
        .collect();
    assert_eq!(
        pending_pairs,
        vec![(user_id, booking_period.clone())],
        "exactly the one past month touched"
    );

    app.state
        .db
        .export_queue
        .delete_entry(user_id, &booking_period)
        .await
        .expect("clear the queue entry");

    // Reversing the booking touches the same archived month again.
    let (st, body) = admin
        .post(
            &format!("/api/v1/flextime-adjustments/{adjustment_id}/reverse"),
            &json!({}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "reverse booking: {body}");
    let pending = app.state.db.export_queue.list_pending().await.unwrap();
    let pending_pairs: Vec<(i64, String)> = pending
        .iter()
        .map(|entry| (entry.user_id, entry.period.clone()))
        .collect();
    assert_eq!(
        pending_pairs,
        vec![(user_id, booking_period)],
        "the reversal must requeue the same month"
    );

    app.cleanup().await;
}
