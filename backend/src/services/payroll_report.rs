//! Monthly payroll report: configuration and data assembly.
//!
//! The report is one PDF per month for the whole company, emailed to the
//! payroll accountant / tax office. It replaces a hand-maintained spreadsheet
//! and therefore contains exactly what payroll needs to file:
//!   * absence days per employee for the selected categories — sick days drive
//!     health-insurance reimbursement, unpaid days reduce the salary payout,
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
use crate::repository::User;
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
    /// Single recipient address (the payroll accountant / tax office).
    pub recipient: String,
    /// Day of month on which the previous month is queued (1-28).
    pub day_of_month: u8,
    /// Absence category slugs whose days are listed, in stored order.
    pub absence_category_slugs: Vec<String>,
    pub include_assistant_hours: bool,
    pub include_employee_hours: bool,
}

impl PayrollReportConfig {
    /// True when the report would contain no section at all. Such a
    /// configuration is rejected on save and skipped by the scheduler, because
    /// mailing an empty document to the tax office helps nobody.
    pub fn has_no_content(&self) -> bool {
        self.absence_category_slugs.is_empty()
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
        recipient: settings::load_setting(pool, settings::PAYROLL_REPORT_RECIPIENT_KEY, "").await?,
        day_of_month: settings::load_setting(pool, settings::PAYROLL_REPORT_DAY_OF_MONTH_KEY, "5")
            .await?
            .parse()
            .unwrap_or(5),
        absence_category_slugs: parse_category_slugs(
            &settings::load_setting(
                pool,
                settings::PAYROLL_REPORT_ABSENCE_CATEGORIES_KEY,
                "sick,unpaid",
            )
            .await?,
        ),
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

/// Split the stored comma-separated slug list, dropping blanks and duplicates
/// while preserving the admin's order.
pub fn parse_category_slugs(stored: &str) -> Vec<String> {
    let mut slugs: Vec<String> = Vec::new();
    for slug in stored.split(',') {
        let slug = slug.trim();
        if slug.is_empty() || slugs.iter().any(|existing| existing == slug) {
            continue;
        }
        slugs.push(slug.to_string());
    }
    slugs
}

/// Serialize a slug list back into the stored comma-separated form.
pub fn format_category_slugs(slugs: &[String]) -> String {
    parse_category_slugs(&slugs.join(",")).join(",")
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

    let absence_rows = if config.absence_category_slugs.is_empty() {
        None
    } else {
        Some(build_absence_rows(app_state, from, to, members, config).await?)
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

/// One row per absence period of a selected category, clamped to the reported
/// month and to the employee's start date.
async fn build_absence_rows(
    app_state: &AppState,
    from: NaiveDate,
    to: NaiveDate,
    members: &[User],
    config: &PayrollReportConfig,
) -> AppResult<Vec<PayrollAbsenceRow>> {
    // Category order in the PDF follows the admin's category sort order, so all
    // sick days stay together, then all unpaid days, and so on.
    let categories = app_state.db.absence_categories.list_all().await?;
    let selected: Vec<(String, String, usize)> = config
        .absence_category_slugs
        .iter()
        .filter_map(|slug| {
            categories
                .iter()
                .enumerate()
                .find(|(_, category)| &category.slug == slug)
                .map(|(index, category)| (category.slug.clone(), category.name.clone(), index))
        })
        .collect();
    if selected.is_empty() {
        return Ok(Vec::new());
    }

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
    fn parse_category_slugs_trims_and_deduplicates() {
        assert_eq!(
            parse_category_slugs(" sick , unpaid ,sick, "),
            vec!["sick".to_string(), "unpaid".to_string()]
        );
        assert!(parse_category_slugs("").is_empty());
        assert!(parse_category_slugs("  ,  ").is_empty());
    }

    #[test]
    fn format_category_slugs_round_trips_the_stored_value() {
        let slugs = vec!["sick".to_string(), "unpaid".to_string()];
        assert_eq!(format_category_slugs(&slugs), "sick,unpaid");
        assert_eq!(parse_category_slugs(&format_category_slugs(&slugs)), slugs);
        assert_eq!(format_category_slugs(&[]), "");
    }

    fn config(
        slugs: &[&str],
        include_assistant_hours: bool,
        include_employee_hours: bool,
    ) -> PayrollReportConfig {
        PayrollReportConfig {
            enabled: true,
            recipient: "payroll@example.com".into(),
            day_of_month: 5,
            absence_category_slugs: slugs.iter().map(|slug| slug.to_string()).collect(),
            include_assistant_hours,
            include_employee_hours,
        }
    }

    #[test]
    fn has_no_content_only_when_every_section_is_off() {
        assert!(config(&[], false, false).has_no_content());
        assert!(!config(&["sick"], false, false).has_no_content());
        assert!(!config(&[], true, false).has_no_content());
        assert!(!config(&[], false, true).has_no_content());
    }

    #[test]
    fn includes_hours_for_splits_assistants_from_everyone_else() {
        let assistants_only = config(&[], true, false);
        assert!(assistants_only.includes_hours_for("assistant"));
        assert!(!assistants_only.includes_hours_for("employee"));
        assert!(!assistants_only.includes_hours_for("team_lead"));

        let employees_only = config(&[], false, true);
        assert!(!employees_only.includes_hours_for("assistant"));
        assert!(employees_only.includes_hours_for("employee"));
        assert!(employees_only.includes_hours_for("admin"));

        let neither = config(&["sick"], false, false);
        assert!(!neither.includes_hours_for("assistant"));
        assert!(!neither.includes_hours_for("employee"));
    }
}
