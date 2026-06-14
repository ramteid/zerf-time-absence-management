use crate::error::AppResult;
use crate::middleware::auth::User;
use crate::services::absences::{
    approve_absence, approve_cancellation_absence, assert_can_access_user, cancel_absence,
    compute_balance, create_absence, enrich_absence_with_metadata, reject_absence,
    reject_cancellation_absence, repo_absence_to_service, require_tracks_time, revoke_absence,
    update_absence, Absence, LeaveBalance, NewAbsence,
};
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::NaiveDate;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct YearQuery {
    pub year: Option<i32>,
}

#[derive(Deserialize)]
pub struct AllQuery {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub status: Option<String>,
}

#[derive(Deserialize)]
pub struct MonthQuery {
    pub month: String,
}

#[derive(Deserialize)]
pub struct BalanceQuery {
    pub year: Option<i32>,
}

#[derive(Deserialize)]
pub struct RejectBody {
    pub reason: String,
}

pub async fn list(
    State(app_state): State<AppState>,
    requester: User,
    Query(query): Query<YearQuery>,
) -> AppResult<Json<Vec<Absence>>> {
    require_tracks_time(&requester)?;
    let year = match query.year {
        Some(value) => value,
        None => crate::services::settings::app_current_year(&app_state.pool).await,
    };
    let year_from = chrono::NaiveDate::from_ymd_opt(year, 1, 1)
        .ok_or_else(|| crate::error::AppError::BadRequest("Invalid year.".into()))?;
    let year_to = chrono::NaiveDate::from_ymd_opt(year, 12, 31)
        .ok_or_else(|| crate::error::AppError::BadRequest("Invalid year.".into()))?;
    let absences = app_state
        .db
        .absences
        .list_for_user(requester.id, year_from, year_to)
        .await?;
    Ok(Json(
        absences.into_iter().map(repo_absence_to_service).collect(),
    ))
}

pub async fn list_all(
    State(app_state): State<AppState>,
    requester: User,
    Query(query): Query<AllQuery>,
) -> AppResult<Json<Vec<Absence>>> {
    if !requester.is_lead() {
        return Err(crate::error::AppError::Forbidden);
    }
    // Enforce a maximum date range to prevent unbounded queries (DoS).
    if let (Some(from), Some(to)) = (query.from, query.to) {
        if from > to {
            return Err(crate::error::AppError::BadRequest(
                "from must not be after to.".into(),
            ));
        }
        if (to - from).num_days() > 366 {
            return Err(crate::error::AppError::BadRequest(
                "Date range must not exceed 366 days.".into(),
            ));
        }
    }
    // Validate status filter against the known set of absence statuses.
    if let Some(ref s) = query.status {
        if ![
            "requested",
            "approved",
            "rejected",
            "cancelled",
            "cancellation_pending",
            "pending_review",
        ]
        .contains(&s.as_str())
        {
            return Err(crate::error::AppError::BadRequest(
                "Invalid status filter.".into(),
            ));
        }
    }
    let absences = app_state
        .db
        .absences
        .list_all(
            requester.is_admin(),
            requester.id,
            query.from,
            query.to,
            query.status.as_deref(),
        )
        .await?;

    let mut mapped: Vec<Absence> = absences.into_iter().map(repo_absence_to_service).collect();
    if query.status.as_deref() == Some("pending_review") {
        let ids: Vec<i64> = mapped.iter().map(|a| a.id).collect();
        let before_data_map =
            crate::services::absences::latest_update_before_data_batch(&app_state, &ids).await?;
        for absence in &mut mapped {
            enrich_absence_with_metadata(absence, &before_data_map);
        }
    }
    Ok(Json(mapped))
}

pub async fn calendar(
    State(app_state): State<AppState>,
    requester: User,
    Query(query): Query<MonthQuery>,
) -> AppResult<Json<Vec<serde_json::Value>>> {
    use chrono::Duration;
    let (year_str, month_str) = query
        .month
        .split_once('-')
        .ok_or_else(|| crate::error::AppError::BadRequest("month=YYYY-MM required".into()))?;
    let year: i32 = year_str
        .parse()
        .map_err(|_| crate::error::AppError::BadRequest("Invalid year".into()))?;
    let month: u32 = month_str
        .parse()
        .map_err(|_| crate::error::AppError::BadRequest("Invalid month".into()))?;
    let from = NaiveDate::from_ymd_opt(year, month, 1)
        .ok_or_else(|| crate::error::AppError::BadRequest("Invalid date".into()))?;
    let next_month_first = if month == 12 {
        let next_year = year
            .checked_add(1)
            .ok_or_else(|| crate::error::AppError::BadRequest("Invalid date".into()))?;
        NaiveDate::from_ymd_opt(next_year, 1, 1)
            .ok_or_else(|| crate::error::AppError::BadRequest("Invalid date".into()))?
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
            .ok_or_else(|| crate::error::AppError::BadRequest("Invalid date".into()))?
    };
    let to = next_month_first - Duration::days(1);
    let scope_user_ids = app_state
        .db
        .absences
        .calendar_scope_user_ids(requester.id, requester.is_admin(), requester.is_lead())
        .await?;
    // HashSet for O(1) "is this entry in the lead's report scope?" lookup
    // in the per-entry comment-visibility decision below. `None` means the
    // requester is an admin (no scope restriction → in-scope for everything).
    let in_scope_ids: Option<std::collections::HashSet<i64>> = scope_user_ids
        .as_ref()
        .map(|ids| ids.iter().copied().collect());
    let calendar_entries = app_state
        .db
        .absences
        .calendar_entries(from, to, scope_user_ids.as_deref())
        .await?;
    let requester_is_lead = requester.is_lead();
    Ok(Json(calendar_entries.into_iter().map(|entry| {
        let is_own_entry = entry.user_id == requester.id;
        // "Owner is in the requester's normal scope" — admins see everyone,
        // leads see their direct reports, non-leads see only themselves.
        // The SQL widens the result set to ALSO include team_visible=TRUE
        // entries from OUT-of-scope users, but we need to remember the
        // distinction here so a lead viewing a non-report's team-visible
        // vacation does NOT also get to read the comment.
        let owner_in_scope = match &in_scope_ids {
            None => true,
            Some(ids) => ids.contains(&entry.user_id),
        };
        // Privacy gate for the displayed kind. Note: the SQL in
        // `calendar_entries` already filters non-lead viewers to
        //   (own entries) OR (team_visible = TRUE)
        // so this check is currently always true. We keep it as a defensive
        // belt-and-suspenders so that any future change to the SQL scope
        // (e.g. allowing admins to peek at cross-team sick leave) can't
        // accidentally leak a kind that should stay masked. The "absent"
        // placeholder is what the frontend treats as the privacy-masked
        // fallback (see MASKED_ABSENCE_COLOR / absenceKindLabel).
        let kind_visible = (requester_is_lead && owner_in_scope) || is_own_entry || entry.team_visible;
        let displayed_kind = if kind_visible { entry.kind.clone() } else { "absent".to_string() };
        // We also pass through the stored category display name so the
        // frontend can render the real label even for INACTIVE categories
        // (which are missing from the active-only `/absence-categories`
        // list that powers the frontend `absenceCategories` store). The
        // name is gated on `kind_visible` so privacy-masked entries don't
        // leak the real category through this side channel.
        let displayed_category_name = if kind_visible {
            Some(entry.category_name.clone())
        } else {
            None
        };
        // Comments may contain personal context ("doctor's appointment",
        // "funeral", ...) that is more sensitive than the category itself.
        // We show comments only to the owner OR to a lead viewing one of
        // their own direct reports — NOT to a lead viewing some other
        // team's team_visible entry, even though they see the kind there.
        // This preserves the pre-B9 comment privacy model exactly.
        let comment_visible = is_own_entry || (requester_is_lead && owner_in_scope);
        serde_json::json!({
            "id": entry.id, "user_id": entry.user_id, "name": format!("{} {}", entry.first_name, entry.last_name),
            "kind": displayed_kind,
            "category_name": displayed_category_name,
            "start_date": entry.start_date, "end_date": entry.end_date,
            "status": entry.status,
            "comment": if comment_visible { entry.comment.clone() } else { None }
        })
    }).collect()))
}

pub async fn create(
    State(app_state): State<AppState>,
    requester: User,
    Json(body): Json<NewAbsence>,
) -> AppResult<Json<Absence>> {
    let created = create_absence(&app_state, &requester, body).await?;
    Ok(Json(created))
}

pub async fn get_one(
    State(app_state): State<AppState>,
    requester: User,
    Path(absence_id): Path<i64>,
) -> AppResult<Json<Absence>> {
    let absence = app_state.db.absences.find_by_id(absence_id).await?;
    // Only the owner or an authorized lead/admin may fetch a single absence.
    if absence.user_id != requester.id {
        // Non-leads cannot view other users' absences at all.
        if !requester.is_lead() {
            return Err(crate::error::AppError::Forbidden);
        }
        // Non-admin leads can only view absences of their direct reports,
        // and cannot view admin-subject absences.
        if !requester.is_admin() {
            let is_report = app_state
                .db
                .users
                .is_direct_report(absence.user_id, requester.id)
                .await?;
            if !is_report {
                return Err(crate::error::AppError::Forbidden);
            }
            // Non-admin leads cannot view admin users' absences (admin-subject rule).
            let target_user = app_state
                .db
                .users
                .find_by_id(absence.user_id)
                .await?
                .ok_or(crate::error::AppError::NotFound)?;
            if crate::roles::is_admin_role(&target_user.role) {
                return Err(crate::error::AppError::Forbidden);
            }
        }
    }
    let mut mapped = repo_absence_to_service(absence);
    let before_data_map =
        crate::services::absences::latest_update_before_data_batch(&app_state, &[mapped.id])
            .await?;
    enrich_absence_with_metadata(&mut mapped, &before_data_map);
    Ok(Json(mapped))
}

pub async fn update(
    State(app_state): State<AppState>,
    requester: User,
    Path(absence_id): Path<i64>,
    Json(body): Json<NewAbsence>,
) -> AppResult<Json<Absence>> {
    let updated = update_absence(&app_state, &requester, absence_id, body).await?;
    Ok(Json(updated))
}

pub async fn cancel(
    State(app_state): State<AppState>,
    requester: User,
    Path(absence_id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    let result = cancel_absence(&app_state, &requester, absence_id).await?;
    Ok(Json(result))
}

pub async fn approve(
    State(app_state): State<AppState>,
    requester: User,
    Path(absence_id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    let result = approve_absence(&app_state, &requester, absence_id).await?;
    Ok(Json(result))
}

pub async fn reject(
    State(app_state): State<AppState>,
    requester: User,
    Path(absence_id): Path<i64>,
    Json(body): Json<RejectBody>,
) -> AppResult<Json<serde_json::Value>> {
    let result = reject_absence(&app_state, &requester, absence_id, &body.reason).await?;
    Ok(Json(result))
}

pub async fn revoke(
    State(app_state): State<AppState>,
    requester: User,
    Path(absence_id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    let result = revoke_absence(&app_state, &requester, absence_id).await?;
    Ok(Json(result))
}

pub async fn approve_cancellation(
    State(app_state): State<AppState>,
    requester: User,
    Path(absence_id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    let result = approve_cancellation_absence(&app_state, &requester, absence_id).await?;
    Ok(Json(result))
}

pub async fn reject_cancellation(
    State(app_state): State<AppState>,
    requester: User,
    Path(absence_id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    let result = reject_cancellation_absence(&app_state, &requester, absence_id).await?;
    Ok(Json(result))
}

pub async fn balance(
    State(app_state): State<AppState>,
    requester: User,
    Path(target_user_id): Path<i64>,
    Query(query): Query<BalanceQuery>,
) -> AppResult<Json<LeaveBalance>> {
    assert_can_access_user(&app_state, &requester, target_user_id).await?;
    // Pure-admin users (tracks_time=false) have no absences or leave balance.
    if target_user_id != requester.id {
        let target_user = app_state
            .db
            .users
            .find_by_id(target_user_id)
            .await?
            .ok_or(crate::error::AppError::NotFound)?;
        if !target_user.tracks_time {
            return Err(crate::error::AppError::Forbidden);
        }
    } else {
        require_tracks_time(&requester)?;
    }
    let year = match query.year {
        Some(value) => {
            if !(1970..=2100).contains(&value) {
                return Err(crate::error::AppError::BadRequest(
                    "Invalid year: out of valid range.".into(),
                ));
            }
            value
        }
        None => crate::services::settings::app_current_year(&app_state.pool).await,
    };
    let balance = compute_balance(&app_state, &requester, target_user_id, year).await?;
    Ok(Json(balance))
}
