//! Monthly payroll report: configuration and data assembly.
//!
//! The report is one PDF per month for the whole company, emailed to the
//! payroll accountant / tax office. It replaces a hand-maintained spreadsheet
//! and therefore contains exactly what payroll needs to file:
//!   * absence days per employee for the categories that are automatically
//!     payroll-relevant (see [`AbsenceCategory::is_payroll_relevant`]) — sick
//!     days drive health-insurance reimbursement, unpaid days reduce the
//!     salary payout,
//!   * working days and worked hours per assistant (and optionally per
//!     employee), which is what assistants are paid by.
//!
//! Scheduling lives in `background::payroll_report`; this module only decides
//! *what* is in the document.

use crate::error::AppResult;
use crate::i18n::Language;
use crate::report_pdf::{
    PayrollAbsenceRow, PayrollHoursRow, PayrollHoursSection, PayrollReportData,
};
use crate::repository::{AbsenceCategory, AbsenceCategoryDb, User};
use crate::roles::is_assistant_role;
use crate::services::reports::MonthExportReadiness;
use crate::services::settings;
use crate::time_calc::count_workdays;
use crate::AppState;
use chrono::{Datelike, NaiveDate};

/// Heading translation key of the assistants' working-hours table.
pub const ASSISTANT_HOURS_HEADING_KEY: &str = "pdf_payroll_assistant_hours_heading";
/// Heading translation key of the employees' working-hours table.
pub const EMPLOYEE_HOURS_HEADING_KEY: &str = "pdf_payroll_employee_hours_heading";

/// Admin-configured content and schedule of the payroll report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayrollReportConfig {
    pub enabled: bool,
    /// Recipient addresses, in stored order. All recipients are equal — every
    /// address goes in the message's `To` header, none is primary/CC.
    pub recipients: Vec<String>,
    /// Day of month on which the previous month is queued (1-28).
    pub day_of_month: u8,
    pub include_assistant_hours: bool,
    pub include_employee_hours: bool,
    /// People the admin deliberately left out of the report. They neither
    /// appear in the document nor hold its delivery up.
    pub excluded_user_ids: Vec<i64>,
}

impl PayrollReportConfig {
    /// True when the report would contain no section at all. Such a
    /// configuration is rejected on save and skipped by the scheduler, because
    /// mailing an empty document to the tax office helps nobody. The absence
    /// section's presence depends on whether any category currently
    /// qualifies as payroll-relevant — pass the result of
    /// [`payroll_relevant_categories`].
    pub fn has_no_content(&self, relevant_categories: &[AbsenceCategory]) -> bool {
        relevant_categories.is_empty()
            && !self.include_assistant_hours
            && !self.include_employee_hours
    }

    /// Whether this user's working days and hours appear in the report.
    /// Drives both the rendered sections and the stricter readiness gate: hours
    /// are only meaningful once every entry behind them is approved.
    pub fn includes_hours_for(&self, role: &str) -> bool {
        if is_assistant_role(role) {
            self.include_assistant_hours
        } else {
            self.include_employee_hours
        }
    }
}

pub async fn load_config(pool: &crate::db::DatabasePool) -> AppResult<PayrollReportConfig> {
    Ok(PayrollReportConfig {
        enabled: settings::load_setting(pool, settings::PAYROLL_REPORT_ENABLED_KEY, "false")
            .await?
            == "true",
        recipients: parse_recipient_list(
            &settings::load_setting(pool, settings::PAYROLL_REPORT_RECIPIENT_KEY, "").await?,
        ),
        day_of_month: settings::load_setting(pool, settings::PAYROLL_REPORT_DAY_OF_MONTH_KEY, "5")
            .await?
            .parse()
            .unwrap_or(5),
        include_assistant_hours: settings::load_setting(
            pool,
            settings::PAYROLL_REPORT_ASSISTANT_HOURS_KEY,
            "true",
        )
        .await?
            == "true",
        include_employee_hours: settings::load_setting(
            pool,
            settings::PAYROLL_REPORT_EMPLOYEE_HOURS_KEY,
            "false",
        )
        .await?
            == "true",
        excluded_user_ids: parse_excluded_ids(
            &settings::load_setting(pool, settings::PAYROLL_REPORT_EXCLUDED_USERS_KEY, "").await?,
        ),
    })
}

/// The people a payroll report for this period actually covers.
///
/// Two groups never appear, and therefore never block delivery either:
///   * **admins** — they are the ones running the system, not staff the payroll
///     accountant files for, so they are dropped unconditionally;
///   * anyone the admin put on the exclusion list.
///
/// Report content, the readiness gate and the dashboard tile all go through
/// this one filter, so what the tile counts is exactly what the PDF contains.
pub fn payroll_members(members: Vec<User>, excluded_user_ids: &[i64]) -> Vec<User> {
    members
        .into_iter()
        .filter(|member| {
            !crate::roles::is_admin_role(&member.role)
                && !excluded_user_ids.contains(&member.id)
        })
        .collect()
}

/// Traffic-light status of one person's month, as shown on the dashboard tile.
pub mod status_value {
    /// Everything submitted and approved — this person is done.
    pub const READY: &str = "ready";
    /// Everything submitted, but an approval or absence decision is missing.
    pub const AWAITING_APPROVAL: &str = "awaiting_approval";
    /// Weeks are still missing, or the data needs an admin's attention.
    pub const NOT_SUBMITTED: &str = "not_submitted";
}

/// One row of the payroll status tile's detail list.
///
/// `user_id` and `name` are `None` for people the requesting team lead is not
/// allowed to see. Their status still counts towards the totals — a lead needs
/// to know whether the month is complete even when the outstanding person is
/// not on their team — but the identity never leaves the server.
#[derive(serde::Serialize)]
pub struct PayrollStatusMember {
    pub user_id: Option<i64>,
    pub name: Option<String>,
    pub status: &'static str,
    pub reason_key: Option<&'static str>,
}

/// Everything the payroll dashboard tile renders for the tracked month.
#[derive(serde::Serialize)]
pub struct PayrollStatus {
    /// False when the payroll report is switched off; the tile stays hidden.
    pub enabled: bool,
    /// Tracked period, "YYYY-MM" — always the previous month.
    pub period: String,
    /// Localized month name, e.g. "Juli 2026".
    pub period_label: String,
    pub from: NaiveDate,
    pub to: NaiveDate,
    /// True once the scheduled delivery for this period has gone out. The tile
    /// is greyed out from that moment until the next month begins.
    pub sent: bool,
    pub day_of_month: u8,
    pub total: usize,
    pub ready: usize,
    pub awaiting_approval: usize,
    pub not_submitted: usize,
    pub members: Vec<PayrollStatusMember>,
}

/// Build the payroll status for the dashboard tile.
///
/// Covers exactly the people the report itself covers (see [`payroll_members`])
/// and judges them with the same gate the send path uses, so "12 of 12 done"
/// on the tile means the next scheduled run will actually deliver.
pub async fn build_status(
    app_state: &AppState,
    requester: &crate::middleware::auth::User,
    language: &Language,
) -> AppResult<PayrollStatus> {
    let config = load_config(&app_state.pool).await?;
    let today = settings::app_today(&app_state.pool).await;
    let period = crate::background::schedule::previous_period(today);
    let (from, to) = crate::background::schedule::period_bounds(&period)?;

    // The tile tracks the previous month until its scheduled delivery went out;
    // afterwards it is done for this month. A manual "Send now" copy does not
    // count — the regular delivery is still outstanding.
    let last_sent = settings::load_setting(
        &app_state.pool,
        settings::PAYROLL_REPORT_LAST_SENT_PERIOD_KEY,
        "",
    )
    .await?;
    let sent = last_sent == period || crate::background::schedule::period_is_after(&last_sent, &period);

    let period_label = crate::i18n::format_month(language, from.year(), from.month());
    if !config.enabled {
        return Ok(PayrollStatus {
            enabled: false,
            period,
            period_label,
            from,
            to,
            sent,
            day_of_month: config.day_of_month,
            total: 0,
            ready: 0,
            awaiting_approval: 0,
            not_submitted: 0,
            members: Vec::new(),
        });
    }

    let members = payroll_members(
        app_state
            .db
            .reports
            .timesheet_members_for_period(from, to)
            .await?,
        &config.excluded_user_ids,
    );
    let evaluated = evaluate_members(app_state, &members, &config, from, to).await?;

    // Team leads only see the names of their own people; everybody else on the
    // tile is counted but anonymized.
    let visible_ids: Option<std::collections::HashSet<i64>> = if requester.is_admin() {
        None
    } else {
        Some(
            app_state
                .db
                .reports
                .active_team_members(requester.id, false)
                .await?
                .into_iter()
                .map(|member| member.id)
                .collect(),
        )
    };

    let mut status = PayrollStatus {
        enabled: true,
        period,
        period_label,
        from,
        to,
        sent,
        day_of_month: config.day_of_month,
        total: evaluated.len(),
        ready: 0,
        awaiting_approval: 0,
        not_submitted: 0,
        members: Vec::with_capacity(evaluated.len()),
    };
    for member in evaluated {
        let value = status_for_reason(member.reason_key);
        match value {
            status_value::READY => status.ready += 1,
            status_value::AWAITING_APPROVAL => status.awaiting_approval += 1,
            _ => status.not_submitted += 1,
        }
        let visible = visible_ids
            .as_ref()
            .is_none_or(|ids| ids.contains(&member.user.id));
        status.members.push(PayrollStatusMember {
            user_id: visible.then_some(member.user.id),
            name: visible
                .then(|| format!("{} {}", member.user.first_name, member.user.last_name)),
            status: value,
            reason_key: member.reason_key,
        });
    }
    Ok(status)
}

/// Map a blocking reason onto the tile's three-colour scale: everything that
/// only waits for a decision is amber, everything else is red.
fn status_for_reason(reason_key: Option<&'static str>) -> &'static str {
    match reason_key {
        None => status_value::READY,
        Some("payroll_report_reason_unapproved_entries")
        | Some("payroll_report_reason_pending_absences") => status_value::AWAITING_APPROVAL,
        Some(_) => status_value::NOT_SUBMITTED,
    }
}

/// One covered person plus why their month is not final yet. `reason_key` is
/// `None` when they are ready to be reported.
pub struct MemberReadiness {
    pub user: User,
    pub reason_key: Option<&'static str>,
}

/// Evaluate the month-finality gate for everyone the report covers.
///
/// Shared by the send path and the dashboard tile so both judge readiness by
/// exactly the same rules — a tile that shows "all done" while the report
/// refuses to go out would be worse than no tile at all.
pub async fn evaluate_members(
    app_state: &AppState,
    members: &[User],
    config: &PayrollReportConfig,
    from: NaiveDate,
    to: NaiveDate,
) -> AppResult<Vec<MemberReadiness>> {
    let mut evaluated = Vec::with_capacity(members.len());
    for member in members {
        // Hours are only payroll-grade once every entry behind them is
        // approved — a still-open or merely submitted month would be paid out
        // too low. Full approval is required exactly when this person's hours
        // are actually part of the report.
        let require_full_approval = config.includes_hours_for(&member.role);
        let readiness = crate::services::reports::month_export_readiness(
            &app_state.pool,
            member,
            from,
            to,
            require_full_approval,
        )
        .await?;
        evaluated.push(MemberReadiness {
            user: member.clone(),
            reason_key: readiness_reason_key(readiness),
        });
    }
    Ok(evaluated)
}

/// Translation key describing why a month is not final, or `None` when it is.
fn readiness_reason_key(readiness: MonthExportReadiness) -> Option<&'static str> {
    match readiness {
        MonthExportReadiness::Ready => None,
        MonthExportReadiness::PreStartContent => Some("payroll_report_reason_pre_start_content"),
        MonthExportReadiness::WeeksNotSubmitted => Some("payroll_report_reason_not_submitted"),
        MonthExportReadiness::PendingAbsenceRequests => {
            Some("payroll_report_reason_pending_absences")
        }
        MonthExportReadiness::UnresolvedTimeEntries => {
            Some("payroll_report_reason_unresolved_entries")
        }
        MonthExportReadiness::UnapprovedTimeEntries => {
            Some("payroll_report_reason_unapproved_entries")
        }
    }
}

/// Split the stored comma-separated user ID list. Blanks, duplicates and
/// non-numeric leftovers are dropped: a user who was hard-deleted after being
/// excluded would otherwise keep a dangling ID in the setting forever, and a
/// stale ID simply matches nobody.
pub fn parse_excluded_ids(stored: &str) -> Vec<i64> {
    let mut ids: Vec<i64> = Vec::new();
    for candidate in stored.split(',') {
        let Ok(id) = candidate.trim().parse::<i64>() else {
            continue;
        };
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

/// Serialize an excluded-user list back into the stored comma-separated form,
/// normalized through the parser so the stored value is always duplicate-free.
pub fn format_excluded_ids(ids: &[i64]) -> String {
    parse_excluded_ids(
        &ids.iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(","),
    )
    .iter()
    .map(|id| id.to_string())
    .collect::<Vec<_>>()
    .join(",")
}

/// Absence categories the payroll report includes automatically — sick-like
/// categories and anything that costs neither vacation nor flextime (see
/// [`AbsenceCategory::is_payroll_relevant`]). Order follows `list_all()`
/// (active first, then sort_order, then name), which is also the order rows
/// appear in the rendered PDF.
pub async fn payroll_relevant_categories(
    pool: &crate::db::DatabasePool,
) -> AppResult<Vec<AbsenceCategory>> {
    let categories = AbsenceCategoryDb::new(pool.clone()).list_all().await?;
    Ok(categories
        .into_iter()
        .filter(AbsenceCategory::is_payroll_relevant)
        .collect())
}

/// Split the stored comma-separated recipient list, dropping blanks and
/// duplicates (case-insensitively) while preserving the admin's order.
pub fn parse_recipient_list(stored: &str) -> Vec<String> {
    let mut recipients: Vec<String> = Vec::new();
    for address in stored.split(',') {
        let address = address.trim();
        if address.is_empty()
            || recipients
                .iter()
                .any(|existing: &String| existing.eq_ignore_ascii_case(address))
        {
            continue;
        }
        recipients.push(address.to_string());
    }
    recipients
}

/// Serialize a recipient list back into the stored comma-separated form.
pub fn format_recipient_list(recipients: &[String]) -> String {
    parse_recipient_list(&recipients.join(",")).join(",")
}

/// Assemble everything the payroll report PDF renders for one period.
///
/// `members` are the people the report actually covers, already narrowed by
/// [`payroll_members`]. For a manual partial send this is only the subset whose
/// month is final; `provisional` then describes what is missing.
pub async fn build_report_data(
    app_state: &AppState,
    from: NaiveDate,
    to: NaiveDate,
    members: &[User],
    config: &PayrollReportConfig,
    language: &Language,
    provisional: Option<crate::report_pdf::ProvisionalNotice>,
) -> AppResult<PayrollReportData> {
    let organization_name =
        settings::load_setting(&app_state.pool, settings::ORGANIZATION_NAME_KEY, "").await?;

    let relevant_categories = payroll_relevant_categories(&app_state.pool).await?;
    let absence_rows = if relevant_categories.is_empty() {
        None
    } else {
        Some(build_absence_rows(app_state, from, to, members, &relevant_categories).await?)
    };

    let mut hours_sections = Vec::new();
    if config.include_assistant_hours {
        hours_sections.push(PayrollHoursSection {
            heading_key: ASSISTANT_HOURS_HEADING_KEY,
            rows: build_hours_rows(app_state, from, to, members, true).await?,
        });
    }
    if config.include_employee_hours {
        hours_sections.push(PayrollHoursSection {
            heading_key: EMPLOYEE_HOURS_HEADING_KEY,
            rows: build_hours_rows(app_state, from, to, members, false).await?,
        });
    }

    Ok(PayrollReportData {
        // `from` is the first day of the reported month, so it carries the
        // period the heading needs without passing the raw "YYYY-MM" string
        // (and its parsing) down into the service layer.
        period_label: crate::i18n::format_month(language, from.year(), from.month()),
        organization_name,
        absence_rows,
        hours_sections,
        provisional,
    })
}

/// One row per absence period of a payroll-relevant category, clamped to the
/// reported month and to the employee's start date.
async fn build_absence_rows(
    app_state: &AppState,
    from: NaiveDate,
    to: NaiveDate,
    members: &[User],
    relevant_categories: &[AbsenceCategory],
) -> AppResult<Vec<PayrollAbsenceRow>> {
    // Category order in the PDF follows `list_all()`'s order, so all sick days
    // stay together, then all unpaid days, and so on.
    let selected: Vec<(String, String, usize)> = relevant_categories
        .iter()
        .enumerate()
        .map(|(index, category)| (category.slug.clone(), category.name.clone(), index))
        .collect();

    let holidays = app_state.db.reports.holiday_set(from, to).await?;

    let mut rows: Vec<(usize, String, PayrollAbsenceRow)> = Vec::new();
    for member in members {
        let absences = app_state
            .db
            .reports
            .approved_absence_rows(member.id, from, to)
            .await?;
        for (start_date, end_date, slug, _stored_name) in absences {
            let Some((_, category_name, category_rank)) = selected
                .iter()
                .find(|(selected_slug, _, _)| selected_slug == &slug)
            else {
                continue;
            };
            // Clamp to the reported month and to the employee's start date:
            // days before the contract started are not payroll-relevant and are
            // hidden everywhere else in the app too.
            let row_from = start_date.max(from).max(member.start_date);
            let row_to = end_date.min(to);
            if row_from > row_to {
                continue;
            }
            let days = count_workdays(row_from, row_to, &holidays, member.workdays_per_week);
            // An absence that only covers non-working days (weekend, holiday)
            // has no payroll effect — leave it out instead of printing a 0 row.
            if days <= 0.0 {
                continue;
            }
            rows.push((
                *category_rank,
                employee_name(member),
                PayrollAbsenceRow {
                    employee: employee_name(member),
                    category: category_name.clone(),
                    from: row_from,
                    to: row_to,
                    days,
                },
            ));
        }
    }

    // Category, then employee, then chronological within one employee.
    rows.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.2.from.cmp(&right.2.from))
    });
    Ok(rows.into_iter().map(|(_, _, row)| row).collect())
}

/// Working days and worked minutes per person of one group (assistants or
/// everyone else). Uses the same month report the timesheet PDF is built from,
/// so the numbers match the archived timesheets exactly.
async fn build_hours_rows(
    app_state: &AppState,
    from: NaiveDate,
    to: NaiveDate,
    members: &[User],
    assistants: bool,
) -> AppResult<Vec<PayrollHoursRow>> {
    let mut rows = Vec::new();
    for member in members {
        if is_assistant_role(&member.role) != assistants {
            continue;
        }
        let auth_user = crate::services::users::repo_user_to_auth_user(member.clone());
        let report = crate::services::reports::build_range_with_user(
            &app_state.pool,
            &auth_user,
            from,
            to,
            "",
        )
        .await?;
        rows.push(PayrollHoursRow {
            employee: employee_name(member),
            // A day counts as worked when it carries approved working time.
            work_days: report.days.iter().filter(|day| day.actual_min > 0).count() as i64,
            minutes: report.actual_min,
        });
    }
    // `members` already arrives ordered by last name; keep that order explicit
    // so the payroll list is alphabetical by surname on every run.
    rows.sort_by(|left, right| left.employee.cmp(&right.employee));
    Ok(rows)
}

/// Surname-first name, the ordering convention payroll lists use.
fn employee_name(user: &User) -> String {
    format!("{}, {}", user.last_name, user.first_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_recipient_list_trims_and_deduplicates_case_insensitively() {
        assert_eq!(
            parse_recipient_list(" a@example.com , B@Example.com ,a@example.com, "),
            vec!["a@example.com".to_string(), "B@Example.com".to_string()]
        );
        assert!(parse_recipient_list("").is_empty());
        assert!(parse_recipient_list("  ,  ").is_empty());
    }

    #[test]
    fn format_recipient_list_round_trips_the_stored_value() {
        let recipients = vec!["a@example.com".to_string(), "b@example.com".to_string()];
        assert_eq!(
            format_recipient_list(&recipients),
            "a@example.com,b@example.com"
        );
        assert_eq!(
            parse_recipient_list(&format_recipient_list(&recipients)),
            recipients
        );
        assert_eq!(format_recipient_list(&[]), "");
    }

    #[test]
    fn parse_excluded_ids_drops_blanks_duplicates_and_junk() {
        assert_eq!(parse_excluded_ids(" 3 , 7,3 , ,x, 11 "), vec![3, 7, 11]);
        assert!(parse_excluded_ids("").is_empty());
        assert!(parse_excluded_ids(" , ").is_empty());
        // A hard-deleted user leaves a stale ID behind; it simply matches nobody.
        assert_eq!(parse_excluded_ids("999999"), vec![999999]);
    }

    #[test]
    fn format_excluded_ids_round_trips_the_stored_value() {
        assert_eq!(format_excluded_ids(&[3, 7, 11]), "3,7,11");
        assert_eq!(format_excluded_ids(&[]), "");
        // Duplicates are normalized away on save.
        assert_eq!(format_excluded_ids(&[3, 3, 7]), "3,7");
        assert_eq!(parse_excluded_ids(&format_excluded_ids(&[3, 7])), vec![3, 7]);
    }

    /// The three-colour scale the dashboard tile paints: anything merely
    /// waiting for a decision is amber, anything still missing data is red.
    #[test]
    fn readiness_maps_onto_the_traffic_light_scale() {
        use status_value::*;
        assert_eq!(status_for_reason(None), READY);
        assert_eq!(
            status_for_reason(Some("payroll_report_reason_unapproved_entries")),
            AWAITING_APPROVAL
        );
        assert_eq!(
            status_for_reason(Some("payroll_report_reason_pending_absences")),
            AWAITING_APPROVAL
        );
        assert_eq!(
            status_for_reason(Some("payroll_report_reason_not_submitted")),
            NOT_SUBMITTED
        );
        assert_eq!(
            status_for_reason(Some("payroll_report_reason_unresolved_entries")),
            NOT_SUBMITTED
        );
        assert_eq!(
            status_for_reason(Some("payroll_report_reason_pre_start_content")),
            NOT_SUBMITTED
        );
    }

    /// Every `MonthExportReadiness` variant must have a reason key, so a new
    /// variant cannot silently show up as "ready" on the tile.
    #[test]
    fn every_non_ready_readiness_has_a_reason() {
        for readiness in [
            MonthExportReadiness::PreStartContent,
            MonthExportReadiness::UnresolvedTimeEntries,
            MonthExportReadiness::PendingAbsenceRequests,
            MonthExportReadiness::WeeksNotSubmitted,
            MonthExportReadiness::UnapprovedTimeEntries,
        ] {
            assert!(
                readiness_reason_key(readiness).is_some(),
                "{readiness:?} must explain itself"
            );
        }
        assert!(readiness_reason_key(MonthExportReadiness::Ready).is_none());
    }

    fn config(include_assistant_hours: bool, include_employee_hours: bool) -> PayrollReportConfig {
        PayrollReportConfig {
            enabled: true,
            recipients: vec!["payroll@example.com".into()],
            day_of_month: 5,
            include_assistant_hours,
            include_employee_hours,
            excluded_user_ids: Vec::new(),
        }
    }

    fn category(
        slug: &str,
        cost_type: &str,
        auto_approve_past: bool,
        unpaid: bool,
    ) -> AbsenceCategory {
        AbsenceCategory {
            id: 1,
            slug: slug.to_string(),
            name: slug.to_string(),
            color: "#000000".to_string(),
            sort_order: 0,
            active: true,
            cost_type: cost_type.to_string(),
            auto_approve_past,
            unpaid,
        }
    }

    #[test]
    fn has_no_content_only_when_every_section_is_off() {
        let sick = category(
            "sick",
            crate::repository::absence_categories::COST_TYPE_NONE,
            true,
            false,
        );
        assert!(config(false, false).has_no_content(&[]));
        assert!(!config(false, false).has_no_content(&[sick]));
        assert!(!config(true, false).has_no_content(&[]));
        assert!(!config(false, true).has_no_content(&[]));
    }

    #[test]
    fn includes_hours_for_splits_assistants_from_everyone_else() {
        let assistants_only = config(true, false);
        assert!(assistants_only.includes_hours_for("assistant"));
        assert!(!assistants_only.includes_hours_for("employee"));
        assert!(!assistants_only.includes_hours_for("team_lead"));

        let employees_only = config(false, true);
        assert!(!employees_only.includes_hours_for("assistant"));
        assert!(employees_only.includes_hours_for("employee"));
        assert!(employees_only.includes_hours_for("admin"));

        let neither = config(false, false);
        assert!(!neither.includes_hours_for("assistant"));
        assert!(!neither.includes_hours_for("employee"));
    }
}
