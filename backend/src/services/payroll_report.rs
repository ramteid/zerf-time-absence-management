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
    })
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
/// `members` are the people the period covers — the same set the timesheet
/// export uses, so archived accounts with data in the month are included and
/// people who only joined later are not.
pub async fn build_report_data(
    app_state: &AppState,
    from: NaiveDate,
    to: NaiveDate,
    members: &[User],
    config: &PayrollReportConfig,
    language: &Language,
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

    fn config(include_assistant_hours: bool, include_employee_hours: bool) -> PayrollReportConfig {
        PayrollReportConfig {
            enabled: true,
            recipients: vec!["payroll@example.com".into()],
            day_of_month: 5,
            include_assistant_hours,
            include_employee_hours,
        }
    }

    fn category(slug: &str, cost_type: &str, auto_approve_past: bool, unpaid: bool) -> AbsenceCategory {
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
