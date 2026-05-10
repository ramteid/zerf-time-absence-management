//! Weekly reopen-request workflow.
//!
//! An employee whose week is fully `submitted` or partially `approved` can
//! request to make the week editable again.  The approver (admin or the
//! configured team-lead) reviews the request.  When the **requester's own**
//! flag `allow_reopen_without_approval` is TRUE, the request is auto-approved
//! immediately and all relevant approvers (designated approver + all admins)
//! receive an informational notification.
//!
//! Approval / auto-approval reopens the week atomically:
//!   * all non-draft entries for `[week_start, week_start+6 days]` are reset
//!     to `'draft'` (audit-logged per entry);
//!   * any open `change_requests` for those entries are auto-approved and
//!     applied before the status reset.

use crate::audit;
use crate::auth::User;
use crate::error::{AppError, AppResult};
use crate::i18n;
use crate::notifications;
use crate::AppState;
use axum::{
    extract::{Path, State},
    Json,
};
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(FromRow, Serialize)]
pub struct ReopenRequest {
    pub id: i64,
    pub user_id: i64,
    pub week_start: NaiveDate,
    /// Set once the request is approved or rejected (NULL while pending).
    pub reviewed_by: Option<i64>,
    pub status: String,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub rejection_reason: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct NewReopen {
    pub week_start: NaiveDate,
}

#[derive(Deserialize)]
pub struct RejectBody {
    pub reason: String,
}

fn assert_monday(d: NaiveDate) -> AppResult<()> {
    if d.weekday() != chrono::Weekday::Mon {
        return Err(AppError::BadRequest(
            "week_start must be a Monday (ISO).".into(),
        ));
    }
    Ok(())
}

#[derive(FromRow)]
struct ChangeOverviewRow {
    entry_date: NaiveDate,
    start_time: String,
    end_time: String,
    old_category_name: String,
    comment: Option<String>,
    new_date: Option<NaiveDate>,
    new_start_time: Option<String>,
    new_end_time: Option<String>,
    new_category_name: Option<String>,
    new_comment: Option<String>,
}

fn hhmm(value: &str) -> String {
    value.chars().take(5).collect()
}

fn change_request_overview_text(
    language: &i18n::Language,
    rows: &[ChangeOverviewRow],
    applied: bool,
) -> String {
    if rows.is_empty() {
        return if language.code() == "de" {
            "Keine offenen Änderungsanträge in dieser Woche.".to_string()
        } else {
            "No open change requests in this week.".to_string()
        };
    }

    let header = if language.code() == "de" {
        if applied {
            "Automatisch übernommene Änderungsanträge:"
        } else {
            "Offene Änderungsanträge für diese Woche:"
        }
    } else if applied {
        "Automatically applied change requests:"
    } else {
        "Open change requests for this week:"
    };

    let mut lines = vec![header.to_string()];
    for row in rows {
        let before_category = i18n::work_category_label(language, &row.old_category_name);
        let after_category = i18n::work_category_label(
            language,
            row.new_category_name
                .as_deref()
                .unwrap_or(&row.old_category_name),
        );
        let before_comment = row.comment.as_deref().unwrap_or("").trim();
        let after_comment = row.new_comment.as_deref().unwrap_or(before_comment).trim();
        let before_comment = if before_comment.is_empty() {
            if language.code() == "de" {
                "(leer)"
            } else {
                "(empty)"
            }
        } else {
            before_comment
        };
        let after_comment = if after_comment.is_empty() {
            if language.code() == "de" {
                "(leer)"
            } else {
                "(empty)"
            }
        } else {
            after_comment
        };
        let base_line = format!(
            "- {} {}-{} ({}) -> {} {}-{} ({})",
            i18n::format_date(language, row.entry_date),
            hhmm(&row.start_time),
            hhmm(&row.end_time),
            before_category,
            i18n::format_date(language, row.new_date.unwrap_or(row.entry_date)),
            hhmm(row.new_start_time.as_deref().unwrap_or(&row.start_time)),
            hhmm(row.new_end_time.as_deref().unwrap_or(&row.end_time)),
            after_category,
        );
        lines.push(base_line);
        if before_comment != after_comment {
            let comment_label = if language.code() == "de" {
                "  Kommentar"
            } else {
                "  Comment"
            };
            lines.push(format!("{comment_label}: {before_comment} -> {after_comment}"));
        }
    }
    lines.join("\n")
}

async fn load_change_request_overview(
    pool: &crate::db::DatabasePool,
    language: &i18n::Language,
    user_id: i64,
    week_start: NaiveDate,
    applied: bool,
) -> String {
    let week_end = week_start + chrono::Duration::days(6);
    let rows = sqlx::query_as::<_, ChangeOverviewRow>(
        "SELECT te.entry_date, te.start_time, te.end_time, \
                c_old.name AS old_category_name, te.comment, \
                cr.new_date, cr.new_start_time, cr.new_end_time, \
                c_new.name AS new_category_name, cr.new_comment \
         FROM change_requests cr \
         JOIN time_entries te ON te.id = cr.time_entry_id \
         LEFT JOIN categories c_old ON c_old.id = te.category_id \
         LEFT JOIN categories c_new ON c_new.id = cr.new_category_id \
         WHERE cr.status='open' AND te.user_id=$1 AND te.entry_date BETWEEN $2 AND $3 \
         ORDER BY te.entry_date, te.start_time, cr.id",
    )
    .bind(user_id)
    .bind(week_start)
    .bind(week_end)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    change_request_overview_text(language, &rows, applied)
}

/// Atomically reopen a week: apply open change_requests for the week's
/// non-draft entries, then reset those entries to draft. Caller is the
/// **acting** user (approver or self); `subject` is the user whose week
/// is being reopened. Returns the affected entry ids and their previous status
/// so the caller can commit the whole state transition first and audit after.
async fn perform_reopen_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    actor_id: i64,
    subject_id: i64,
    week_start: NaiveDate,
) -> AppResult<Vec<(i64, String)>> {
    let week_end = week_start + chrono::Duration::days(6);

    let affected: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, status FROM time_entries \
         WHERE user_id=$1 AND entry_date BETWEEN $2 AND $3 AND status<>'draft' FOR UPDATE",
    )
    .bind(subject_id)
    .bind(week_start)
    .bind(week_end)
    .fetch_all(&mut **tx)
    .await?;

    if affected.is_empty() {
        return Err(AppError::BadRequest(
            "Nothing to reopen — this week has no submitted or approved entries.".into(),
        ));
    }

    // Auto-apply open change requests for these entries.
    let entry_ids: Vec<i64> = affected.iter().map(|(id, _)| *id).collect();
    sqlx::query(
        "UPDATE time_entries te \
         SET entry_date=COALESCE(cr.new_date, te.entry_date), \
             start_time=COALESCE(cr.new_start_time, te.start_time), \
             end_time=COALESCE(cr.new_end_time, te.end_time), \
             category_id=COALESCE(cr.new_category_id, te.category_id), \
             comment=CASE WHEN cr.new_comment IS NOT NULL THEN NULLIF(cr.new_comment,'') ELSE te.comment END, \
             updated_at=CURRENT_TIMESTAMP \
         FROM change_requests cr \
         WHERE cr.status='open' AND te.id = cr.time_entry_id AND te.id = ANY($1)",
    )
    .bind(&entry_ids)
    .execute(&mut **tx)
    .await?;

    sqlx::query(
        "UPDATE change_requests \
         SET status='approved', \
             reviewed_by=$1, \
             reviewed_at=CURRENT_TIMESTAMP, \
             rejection_reason=NULL \
         WHERE status='open' AND time_entry_id = ANY($2)",
    )
    .bind(actor_id)
    .bind(&entry_ids)
    .execute(&mut **tx)
    .await?;

    // Reset all affected entries to draft.  We filter by their original IDs
    // (not by date range) because the CR-apply step above may have moved some
    // entries to a date outside the original week; a date-range filter would
    // silently miss those and leave them in submitted/approved status.
    sqlx::query(
        "UPDATE time_entries SET status='draft', submitted_at=NULL, reviewed_by=NULL, \
         reviewed_at=NULL, rejection_reason=NULL, updated_at=CURRENT_TIMESTAMP \
         WHERE id = ANY($1)",
    )
    .bind(&entry_ids)
    .execute(&mut **tx)
    .await?;

    Ok(affected)
}

async fn audit_reopened_entries(
    pool: &crate::db::DatabasePool,
    actor_id: i64,
    affected: &[(i64, String)],
) {
    for (entry_id, prev_status) in affected {
        audit::log(
            pool,
            actor_id,
            "reopened",
            "time_entries",
            *entry_id,
            Some(serde_json::json!({"status": prev_status})),
            Some(serde_json::json!({"status":"draft"})),
        )
        .await;
    }
}

/// Collect all user-ids that should be notified as "approver" for a reopen
/// request created by `requester`.  Rules:
///
/// | Requester role | Scenario                            | Notified set                          |
/// |----------------|-------------------------------------|---------------------------------------|
/// | employee       | any                                 | designated approver + all admins      |
/// | team_lead      | has designated approver             | that approver + all admins            |
/// | admin          | any                                 | all other admins                      |
///
/// BTreeSet deduplicates (e.g. when the designated approver IS an admin).
/// Non-admin requesters are excluded from the result.
async fn approver_ids_to_notify(pool: &crate::db::DatabasePool, requester: &User) -> Vec<i64> {
    let mut ids: std::collections::BTreeSet<i64> = Default::default();
    // Get all assigned approvers for this user
    let approver_ids = crate::auth::approval_recipient_ids(pool, requester).await;
    ids.extend(approver_ids);
    if let Ok(admin_ids) =
        sqlx::query_scalar::<_, i64>("SELECT id FROM users WHERE active=TRUE AND role='admin'")
            .fetch_all(pool)
            .await
    {
        ids.extend(admin_ids);
    }
    // Only exclude the requester when they are NOT an admin.  An admin who
    // requests a reopen for their own week still needs a notification so
    // they can approve it from the dashboard (especially when they are the
    // only admin).
    if !requester.is_admin() {
        ids.remove(&requester.id);
    }
    ids.into_iter().collect()
}

async fn notification_language(pool: &crate::db::DatabasePool) -> i18n::Language {
    match i18n::load_ui_language(pool).await {
        Ok(language) => language,
        Err(e) => {
            tracing::warn!(target:"zerf::reopen", "load notification language failed: {e}");
            i18n::Language::default()
        }
    }
}

pub async fn create(
    State(app_state): State<AppState>,
    requester: User,
    Json(body): Json<NewReopen>,
) -> AppResult<Json<serde_json::Value>> {
    assert_monday(body.week_start)?;
    let week_end = body.week_start + chrono::Duration::days(6);

    // Empty-week / nothing-to-reopen guard: only weeks with at least one
    // non-draft entry are eligible.
    let non_draft_entry_count = app_state
        .db
        .reopen_requests
        .count_non_draft_entries(requester.id, body.week_start, week_end)
        .await?;
    if non_draft_entry_count == 0 {
        return Err(AppError::BadRequest(
            "Nothing to reopen — this week has no submitted or approved entries.".into(),
        ));
    }

    // Reject duplicate pending request (DB also has a unique partial index).
    let existing_pending_id = app_state
        .db
        .reopen_requests
        .find_pending_request_id(requester.id, body.week_start)
        .await?;
    if let Some(existing_request_id) = existing_pending_id {
        return Err(AppError::Conflict(format!(
            "A pending reopen request already exists (id {existing_request_id})."
        )));
    }

    // Determine flow:
    //   * User has `allow_reopen_without_approval=TRUE` → auto_approved
    //   * Otherwise → pending, notify all approvers
    let should_auto_approve = requester.allow_reopen_without_approval;

    // Verify that at least one approver is available.
    let approvers = crate::auth::approval_recipient_ids(&app_state.pool, &requester).await;
    if approvers.is_empty() {
        return Err(AppError::Conflict(
            "No valid approver is available for this reopen request.".into(),
        ));
    }

    let initial_status = if should_auto_approve {
        "auto_approved"
    } else {
        "pending"
    };

    // Collect all users that should be notified as approvers (before the DB
    // insert, so we can reuse the result for both auto and pending paths).
    let approver_ids_for_notification = approver_ids_to_notify(&app_state.pool, &requester).await;
    let language = notification_language(&app_state.pool).await;
    let week_label = i18n::format_week_label(&language, body.week_start);
    let pending_change_overview =
        load_change_request_overview(&app_state.pool, &language, requester.id, body.week_start, false)
            .await;
    let applied_change_overview =
        load_change_request_overview(&app_state.pool, &language, requester.id, body.week_start, true)
            .await;

        let (new_request_id, entries_reopened, reopened_entries): (i64, i64, Vec<(i64, String)>) =
        if should_auto_approve {
            let mut transaction = app_state.pool.begin().await?;
            let insert_result: (i64, DateTime<Utc>) = sqlx::query_as(
                "INSERT INTO reopen_requests(user_id, week_start, status, reviewed_by, reviewed_at) \
                 VALUES ($1,$2,$3,$4, CURRENT_TIMESTAMP) \
                 RETURNING id, created_at",
            )
            .bind(requester.id)
            .bind(body.week_start)
            .bind(initial_status)
            .bind(requester.id)
            .fetch_one(&mut *transaction)
            .await
            .map_err(|e| {
                tracing::warn!(target:"zerf::reopen", "create reopen failed: {e}");
                AppError::Conflict("A pending request for this week already exists.".into())
            })?;
            let affected = perform_reopen_in_tx(
                &mut transaction,
                requester.id,
                requester.id,
                body.week_start,
            )
            .await?;
            transaction.commit().await?;
            (insert_result.0, affected.len() as i64, affected)
        } else {
            let insert_result: (i64, DateTime<Utc>) = sqlx::query_as(
                "INSERT INTO reopen_requests(user_id, week_start, status) \
                 VALUES ($1,$2,$3) \
                 RETURNING id, created_at",
            )
            .bind(requester.id)
            .bind(body.week_start)
            .bind(initial_status)
            .fetch_one(&app_state.pool)
            .await
            .map_err(|e| {
                tracing::warn!(target:"zerf::reopen", "create reopen failed: {e}");
                AppError::Conflict("A pending request for this week already exists.".into())
            })?;
            (insert_result.0, 0, vec![])
        };

    if should_auto_approve {
        audit_reopened_entries(&app_state.pool, requester.id, &reopened_entries).await;
    }

    audit::log(
        &app_state.pool,
        requester.id,
        "created",
        "reopen_requests",
        new_request_id,
        None,
        Some(serde_json::json!({
            "week_start": body.week_start,
            "status": initial_status,
        })),
    )
    .await;

    let requester_full_name = format!("{} {}", requester.first_name, requester.last_name);

    if should_auto_approve {
        // Notify the requester that their week was reopened automatically.
        notifications::create_translated(
            &app_state,
            &language,
            requester.id,
            "reopen_auto_approved",
            "reopen_auto_approved_title",
            "reopen_auto_approved_body",
            vec![
                ("week_label", week_label.clone()),
                ("change_request_overview", applied_change_overview.clone()),
            ],
            Some("reopen_request"),
            Some(new_request_id),
        )
        .await;
        // Notify each approver that the reopen was auto-approved (informational).
        for approver_id in &approver_ids_for_notification {
            notifications::create_translated(
                &app_state,
                &language,
                *approver_id,
                "reopen_auto_approved_notice",
                "reopen_auto_approved_notice_title",
                "reopen_auto_approved_notice_body",
                vec![
                    ("requester_name", requester_full_name.clone()),
                    ("week_label", week_label.clone()),
                    ("change_request_overview", applied_change_overview.clone()),
                ],
                Some("reopen_request"),
                Some(new_request_id),
            )
            .await;
        }
        Ok(Json(serde_json::json!({
            "ok": true,
            "id": new_request_id,
            "status": initial_status,
            "auto_approved": true,
            "entries_reopened": entries_reopened,
        })))
    } else {
        // Notify all approvers that a manual reopen request is pending.
        for approver_id in &approver_ids_for_notification {
            notifications::create_translated(
                &app_state,
                &language,
                *approver_id,
                "reopen_request_created",
                "reopen_request_created_title",
                "reopen_request_created_body",
                vec![
                    ("requester_name", requester_full_name.clone()),
                    ("week_label", week_label.clone()),
                    ("change_request_overview", pending_change_overview.clone()),
                ],
                Some("reopen_request"),
                Some(new_request_id),
            )
            .await;
        }
        Ok(Json(serde_json::json!({
            "ok": true,
            "id": new_request_id,
            "status": initial_status,
            "auto_approved": false,
        })))
    }
}

fn repo_rr_to_service(r: crate::repository::ReopenRequest) -> ReopenRequest {
    ReopenRequest {
        id: r.id,
        user_id: r.user_id,
        week_start: r.week_start,
        reviewed_by: r.reviewed_by,
        status: r.status,
        reviewed_at: r.reviewed_at,
        rejection_reason: r.rejection_reason,
        created_at: r.created_at,
    }
}

pub async fn list_mine(
    State(app_state): State<AppState>,
    requester: User,
) -> AppResult<Json<Vec<ReopenRequest>>> {
    let rrs = app_state.db.reopen_requests.list_mine(requester.id).await?;
    Ok(Json(rrs.into_iter().map(repo_rr_to_service).collect()))
}

pub async fn list_pending(
    State(app_state): State<AppState>,
    requester: User,
) -> AppResult<Json<Vec<ReopenRequest>>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    let rrs = if requester.is_admin() {
        app_state.db.reopen_requests.list_pending_admin().await?
    } else {
        app_state.db.reopen_requests.list_pending_for_lead(requester.id).await?
    };
    Ok(Json(rrs.into_iter().map(repo_rr_to_service).collect()))
}

async fn load_pending(app_state: &AppState, id: i64) -> AppResult<ReopenRequest> {
    let rr = app_state.db.reopen_requests.find_by_id(id).await?;
    Ok(repo_rr_to_service(rr))
}

/// If an admin acted on a request, notify all assigned team leads for the request's
/// user so they know the item left their pending queue.
#[allow(clippy::too_many_arguments)]
async fn notify_leads_if_admin_acted(
    app_state: &AppState,
    language: &i18n::Language,
    requester: &User,
    request_user_id: i64,
    request_id: i64,
    action_key: &str,
    action_title_key: &str,
    action_body_key: &str,
    week_label: String,
    change_request_overview: String,
    extra_params: Vec<(&'static str, String)>,
) {
    if !requester.is_admin() {
        return;
    }
    // Fetch all assigned team leads for the request's user (excluding the acting admin).
    let lead_ids: Vec<i64> = match sqlx::query_scalar::<_, i64>(
        "SELECT ua.approver_id FROM user_approvers ua \
         JOIN users u ON u.id = ua.approver_id \
         WHERE ua.user_id = $1 AND u.active = TRUE AND u.role = 'team_lead' AND ua.approver_id != $2",
    )
    .bind(request_user_id)
    .bind(requester.id)
    .fetch_all(&app_state.pool)
    .await
    {
        Ok(ids) => ids,
        Err(_) => return,
    };
    if lead_ids.is_empty() {
        return;
    }
    let employee_full_name: String =
        sqlx::query_scalar("SELECT first_name || ' ' || last_name FROM users WHERE id=$1")
            .bind(request_user_id)
            .fetch_optional(&app_state.pool)
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| format!("User {request_user_id}"));
    let mut params = vec![
        ("requester_name", employee_full_name),
        ("week_label", week_label),
        ("change_request_overview", change_request_overview),
    ];
    params.extend(extra_params);
    for lead_id in lead_ids {
        notifications::create_translated(
            app_state,
            language,
            lead_id,
            action_key,
            action_title_key,
            action_body_key,
            params.clone(),
            Some("reopen_request"),
            Some(request_id),
        )
        .await;
    }
}

pub async fn approve(
    State(app_state): State<AppState>,
    requester: User,
    Path(request_id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    let mut transaction = app_state.pool.begin().await?;
    let reopen_request: ReopenRequest = sqlx::query_as(
        "SELECT id, user_id, week_start, reviewed_by, status, reviewed_at, \
         rejection_reason, created_at \
         FROM reopen_requests WHERE id=$1 FOR UPDATE",
    )
    .bind(request_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    if reopen_request.status != "pending" {
        return Err(AppError::BadRequest("Request is not pending.".into()));
    }
    if !requester.is_admin() {
        let is_assigned_approver: Option<bool> = sqlx::query_scalar(
            "SELECT TRUE FROM user_approvers WHERE user_id = $1 AND approver_id = $2",
        )
        .bind(reopen_request.user_id)
        .bind(requester.id)
        .fetch_optional(&mut *transaction)
        .await?;
        if is_assigned_approver.is_none() {
            return Err(AppError::Forbidden);
        }
    }
    // Team leads must not act on reopen requests from admin users.
    if !requester.is_admin() {
        let is_admin_user: Option<bool> =
            sqlx::query_scalar("SELECT TRUE FROM users WHERE id = $1 AND role = 'admin'")
                .bind(reopen_request.user_id)
                .fetch_optional(&mut *transaction)
                .await?;
        if is_admin_user.is_some() {
            return Err(AppError::Forbidden);
        }
    }
    let language = notification_language(&app_state.pool).await;
    let week_label = i18n::format_week_label(&language, reopen_request.week_start);
    let applied_change_overview = load_change_request_overview(
        &app_state.pool,
        &language,
        reopen_request.user_id,
        reopen_request.week_start,
        true,
    )
    .await;
    let reopened_entries = perform_reopen_in_tx(
        &mut transaction,
        requester.id,
        reopen_request.user_id,
        reopen_request.week_start,
    )
    .await?;
    sqlx::query(
        "UPDATE reopen_requests SET status='approved', reviewed_by=$2, reviewed_at=CURRENT_TIMESTAMP \
         WHERE id=$1 AND status='pending'",
    )
    .bind(request_id)
    .bind(requester.id)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    audit_reopened_entries(&app_state.pool, requester.id, &reopened_entries).await;
    let entries_reopened = reopened_entries.len() as i64;
    audit::log(
        &app_state.pool,
        requester.id,
        "approved",
        "reopen_requests",
        request_id,
        Some(serde_json::to_value(&reopen_request).unwrap()),
        Some(serde_json::json!({"status": "approved"})),
    )
    .await;
    // Notify the employee whose week was reopened.
    notifications::create_translated(
        &app_state,
        &language,
        reopen_request.user_id,
        "reopen_approved",
        "reopen_approved_title",
        "reopen_approved_body",
        vec![
            ("week_label", week_label.clone()),
            ("change_request_overview", applied_change_overview.clone()),
        ],
        Some("reopen_request"),
        Some(request_id),
    )
    .await;
    // If an admin acted, notify all assigned team leads for this user so they
    // know the item left their pending queue.
    notify_leads_if_admin_acted(
        &app_state,
        &language,
        &requester,
        reopen_request.user_id,
        request_id,
        "reopen_approved_by_admin",
        "reopen_approved_by_admin_title",
        "reopen_approved_by_admin_body",
        week_label,
        applied_change_overview,
        vec![],
    )
    .await;
    Ok(Json(
        serde_json::json!({ "ok": true, "entries_reopened": entries_reopened }),
    ))
}

pub async fn reject(
    State(app_state): State<AppState>,
    requester: User,
    Path(request_id): Path<i64>,
    Json(body): Json<RejectBody>,
) -> AppResult<Json<serde_json::Value>> {
    if !requester.is_lead() {
        return Err(AppError::Forbidden);
    }
    let rejection_reason = body.reason.trim();
    if rejection_reason.is_empty() {
        return Err(AppError::BadRequest("Reason required.".into()));
    }
    if rejection_reason.len() > 2000 {
        return Err(AppError::BadRequest("Reason too long.".into()));
    }
    let reopen_request = load_pending(&app_state, request_id).await?;
    if reopen_request.status != "pending" {
        return Err(AppError::BadRequest("Request is not pending.".into()));
    }
    if !requester.is_admin() {
        let is_assigned_approver: Option<bool> = sqlx::query_scalar(
            "SELECT TRUE FROM user_approvers WHERE user_id = $1 AND approver_id = $2",
        )
        .bind(reopen_request.user_id)
        .bind(requester.id)
        .fetch_optional(&app_state.pool)
        .await?;
        if is_assigned_approver.is_none() {
            return Err(AppError::Forbidden);
        }
    }
    // Team leads must not act on reopen requests from admin users.
    if !requester.is_admin() {
        let is_admin_user: Option<bool> =
            sqlx::query_scalar("SELECT TRUE FROM users WHERE id = $1 AND role = 'admin'")
                .bind(reopen_request.user_id)
                .fetch_optional(&app_state.pool)
                .await?;
        if is_admin_user.is_some() {
            return Err(AppError::Forbidden);
        }
    }
    // Use optimistic locking: only proceed if status is still 'pending'.
    let rows_claimed = sqlx::query(
        "UPDATE reopen_requests SET status='rejected', reviewed_by=$2, reviewed_at=CURRENT_TIMESTAMP, \
         rejection_reason=$3 WHERE id=$1 AND status='pending'",
    )
    .bind(request_id)
    .bind(requester.id)
    .bind(rejection_reason)
    .execute(&app_state.pool)
    .await?
    .rows_affected();
    if rows_claimed == 0 {
        return Err(AppError::Conflict(
            "Request was already resolved by someone else.".into(),
        ));
    }
    audit::log(
        &app_state.pool,
        requester.id,
        "rejected",
        "reopen_requests",
        request_id,
        Some(serde_json::to_value(&reopen_request).unwrap()),
        Some(serde_json::json!({ "status": "rejected", "reason": rejection_reason })),
    )
    .await;
    let language = notification_language(&app_state.pool).await;
    let week_label = i18n::format_week_label(&language, reopen_request.week_start);
    let pending_change_overview = load_change_request_overview(
        &app_state.pool,
        &language,
        reopen_request.user_id,
        reopen_request.week_start,
        false,
    )
    .await;
    // Notify the employee whose reopen request was rejected.
    notifications::create_translated(
        &app_state,
        &language,
        reopen_request.user_id,
        "reopen_rejected",
        "reopen_rejected_title",
        "reopen_rejected_body",
        vec![
            ("week_label", week_label.clone()),
            ("change_request_overview", pending_change_overview.clone()),
            ("reason", rejection_reason.to_string()),
        ],
        Some("reopen_request"),
        Some(request_id),
    )
    .await;
    // Symmetric with approve: if an admin rejected a request, notify all assigned
    // team leads for this user so they know the item left their queue.
    notify_leads_if_admin_acted(
        &app_state,
        &language,
        &requester,
        reopen_request.user_id,
        request_id,
        "reopen_rejected_by_admin",
        "reopen_rejected_by_admin_title",
        "reopen_rejected_by_admin_body",
        week_label,
        pending_change_overview,
        vec![("reason", rejection_reason.to_string())],
    )
    .await;
    Ok(Json(serde_json::json!({ "ok": true })))
}
