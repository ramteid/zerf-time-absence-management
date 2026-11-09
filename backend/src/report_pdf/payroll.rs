//! Company-wide monthly payroll report PDF.
//!
//! This is the document that gets emailed to the payroll accountant / tax
//! office once a month. It answers exactly the two questions they file with:
//!   * which absence days occurred per employee (sick days drive
//!     health-insurance reimbursement, unpaid days reduce the salary payout),
//!   * how many days and hours each assistant (and optionally each employee)
//!     actually worked.
//!
//! Column layouts (both sum to the 180 mm content width):
//!   Absences: Employee 60 | Category 40 | From 25 | To 25 | Days 30
//!   Hours:    Employee 70 | Work days 30 | Hours 40 | Hours (decimal) 40
//!   Late:     Employee 70 | Date 30 | Hours 40 | Hours (decimal) 40

use super::{format_minutes, format_signed_minutes, Align, Column, Renderer};
use crate::i18n::{self, Language};
use chrono::{Datelike, NaiveDate};
use std::collections::BTreeMap;

/// Absence table: one row per absence period, clamped to the reported month.
/// All columns are left-aligned so data rows line up under the (always
/// left-aligned) header labels — see [`Renderer::draw_table_header`], and the
/// same rule in [`HOURS_COLUMNS`].
const ABSENCE_COLUMNS: &[Column] = &[
    Column {
        header_key: "pdf_payroll_column_employee",
        width_mm: 52.0,
        align: Align::Left,
    },
    Column {
        header_key: "pdf_column_category",
        width_mm: 35.0,
        align: Align::Left,
    },
    Column {
        header_key: "pdf_payroll_column_from",
        width_mm: 22.0,
        align: Align::Left,
    },
    Column {
        header_key: "pdf_payroll_column_to",
        width_mm: 22.0,
        align: Align::Left,
    },
    Column {
        header_key: "pdf_payroll_column_days",
        width_mm: 24.0,
        align: Align::Left,
    },
    Column {
        header_key: "pdf_payroll_column_medical_certificate",
        width_mm: 25.0,
        align: Align::Left,
    },
];

/// Index of the `Days` column in [`ABSENCE_COLUMNS`].
const ABSENCE_DAYS_COLUMN: usize = 4;
/// Index of the `AU` (medical certificate) column in [`ABSENCE_COLUMNS`].
const ABSENCE_MEDICAL_CERTIFICATE_COLUMN: usize = 5;

const CANCELLED_ABSENCE_COLUMNS: &[Column] = &[
    Column {
        header_key: "pdf_payroll_column_employee",
        width_mm: 60.0,
        align: Align::Left,
    },
    Column {
        header_key: "pdf_column_category",
        width_mm: 60.0,
        align: Align::Left,
    },
    Column {
        header_key: "pdf_payroll_column_from",
        width_mm: 30.0,
        align: Align::Left,
    },
    Column {
        header_key: "pdf_payroll_column_to",
        width_mm: 30.0,
        align: Align::Left,
    },
];

const REVIEW_ABSENCE_COLUMNS: &[Column] = &[
    Column {
        header_key: "pdf_payroll_column_employee",
        width_mm: 50.0,
        align: Align::Left,
    },
    Column {
        header_key: "pdf_column_category",
        width_mm: 50.0,
        align: Align::Left,
    },
    Column {
        header_key: "pdf_payroll_column_from",
        width_mm: 25.0,
        align: Align::Left,
    },
    Column {
        header_key: "pdf_payroll_column_to",
        width_mm: 25.0,
        align: Align::Left,
    },
    Column {
        header_key: "pdf_payroll_column_medical_certificate",
        width_mm: 30.0,
        align: Align::Left,
    },
];

/// Hours table: one row per person in the section. All columns are
/// left-aligned so data rows line up under the (always left-aligned) header
/// labels — see [`Renderer::draw_table_header`].
const HOURS_COLUMNS: &[Column] = &[
    Column {
        header_key: "pdf_payroll_column_employee",
        width_mm: 70.0,
        align: Align::Left,
    },
    Column {
        header_key: "pdf_payroll_column_work_days",
        width_mm: 30.0,
        align: Align::Left,
    },
    Column {
        header_key: "pdf_payroll_column_hours",
        width_mm: 40.0,
        align: Align::Left,
    },
    Column {
        header_key: "pdf_payroll_column_hours_decimal",
        width_mm: 40.0,
        align: Align::Left,
    },
];

const HOURS_WORK_DAYS_COLUMN: usize = 1;
const HOURS_HOURS_COLUMN: usize = 2;
const HOURS_DECIMAL_COLUMN: usize = 3;

/// Corrections table: one row per person and calendar month whose approved
/// net time changed after that month was reported. The month tells payroll
/// which already-filed month the value belongs to.
const LATE_ENTRY_COLUMNS: &[Column] = &[
    Column {
        header_key: "pdf_payroll_column_employee",
        width_mm: 62.0,
        align: Align::Left,
    },
    Column {
        header_key: "pdf_payroll_column_month",
        width_mm: 26.0,
        align: Align::Left,
    },
    Column {
        header_key: "pdf_payroll_column_hours",
        width_mm: 24.0,
        align: Align::Left,
    },
    Column {
        header_key: "pdf_payroll_column_hours_decimal",
        width_mm: 32.0,
        align: Align::Left,
    },
    Column {
        header_key: "pdf_payroll_column_month_total",
        width_mm: 36.0,
        align: Align::Left,
    },
];

const LATE_ENTRY_MONTH_COLUMN: usize = 1;
const LATE_ENTRY_HOURS_COLUMN: usize = 2;
const LATE_ENTRY_DECIMAL_COLUMN: usize = 3;
const LATE_ENTRY_TOTAL_COLUMN: usize = 4;

/// The net correction of one person for one calendar month, with the month's
/// new total.
struct LateEntryMonth<'a> {
    employee: &'a str,
    year: i32,
    month: u32,
    minutes: i64,
    total_minutes: i64,
}

/// Sum the day-level corrections per person and month, ordered by name and
/// then by month. Payroll books hours per month, so the single days do not
/// matter. A month whose changes cancel out (a shift moved within it) needs no
/// adjustment and is left out.
fn sum_late_entries_per_month(rows: &[PayrollLateEntryRow]) -> Vec<LateEntryMonth<'_>> {
    // Per key: the summed correction and the month's total, which all rows of
    // the key share.
    let mut totals: BTreeMap<(&str, i64, i32, u32), (i64, i64)> = BTreeMap::new();
    for row in rows {
        let key = (
            row.employee.as_str(),
            row.user_id,
            row.date.year(),
            row.date.month(),
        );
        totals.entry(key).or_insert((0, row.month_total_minutes)).0 += row.minutes;
    }
    totals
        .into_iter()
        .filter(|(_, (minutes, _))| *minutes != 0)
        .map(
            |((employee, _, year, month), (minutes, total_minutes))| LateEntryMonth {
                employee,
                year,
                month,
                minutes,
                total_minutes,
            },
        )
        .collect()
}

/// One absence period of one employee within the reported month.
pub struct PayrollAbsenceRow {
    /// Not rendered; retained so delivery can persist an exact, access-aware
    /// dashboard snapshot of the document.
    pub user_id: i64,
    pub employee: String,
    /// Display name of the absence category (localized by the caller).
    pub category: String,
    /// Absence start, clamped to the reported month.
    pub from: NaiveDate,
    /// Absence end, clamped to the reported month.
    pub to: NaiveDate,
    /// Contract workdays covered by the clamped range (holidays excluded).
    pub days: f64,
    /// Whether a medical certificate (AU) is required for this absence, per
    /// `services::medical_certificate`. `None` when the category does not
    /// track this at all (renders as "–" rather than "No").
    pub medical_certificate_required: Option<bool>,
}

/// Working days and worked minutes of one person within the reported month.
pub struct PayrollHoursRow {
    /// Not rendered; retained for the delivered dashboard snapshot.
    pub user_id: i64,
    pub employee: String,
    pub work_days: i64,
    pub minutes: i64,
}

/// One "working days and hours" table with its own heading.
pub struct PayrollHoursSection {
    /// Translation key of the section heading.
    pub heading_key: &'static str,
    pub rows: Vec<PayrollHoursRow>,
}

/// One signed correction to a day in an already-reported month.
///
/// It carries its own date rather than being folded into the current month's
/// totals. Positive minutes add time and negative minutes reduce it.
pub struct PayrollLateEntryRow {
    /// Not rendered; retained for the delivered dashboard snapshot.
    pub user_id: i64,
    pub employee: String,
    /// The day actually worked — always before the reported month.
    pub date: NaiveDate,
    pub minutes: i64,
    /// The person's approved net total for the month of `date`, with this
    /// correction included. Every row of one person and month carries the same
    /// value.
    pub month_total_minutes: i64,
}

/// One signed day total the assembled report declares to payroll.
///
/// This is deliberately not rendered itself. It travels with the finished
/// document so the sender records exactly the values the PDF showed, instead
/// of attempting to derive them again from mutable time entries after SMTP has
/// accepted the message.
pub struct PayrollDeclaredWorkDay {
    pub user_id: i64,
    pub date: NaiveDate,
    pub minutes: i64,
}

/// One older person-day whose correction the assembled report printed.
///
/// This stays separate from [`PayrollDeclaredWorkDay`]: a pre-ledger day must
/// remain on the legacy entry-marker fallback, but its newly printed row still
/// has to be marked so it does not repeat.
pub struct PayrollCarriedWorkDay {
    pub user_id: i64,
    pub date: NaiveDate,
}

/// One person the report had to leave out, with the reason in the report
/// language.
pub struct PayrollOmittedPerson {
    pub name: String,
    /// Translation key of the reason, e.g. `payroll_report_reason_not_submitted`.
    pub reason_key: &'static str,
}

/// Marks a report that covers only part of the staff, because an admin sent it
/// before everyone had finished their month.
///
/// Without this, a partial report is indistinguishable from a complete one and
/// the payroll accountant would file short hours as if they were final — so the
/// notice is rendered prominently at the top of the document, not as a footnote.
pub struct ProvisionalNotice {
    /// People actually covered by the tables below.
    pub included: usize,
    /// People the month covers in total (included + omitted).
    pub total: usize,
    pub omitted: Vec<PayrollOmittedPerson>,
    /// True when the reported month is still running, so the figures are a
    /// snapshot of what was approved up to the send date. The caveat is then
    /// the unfinished *month*, not unfinished people — printing "3 of 12
    /// people included" would misrepresent that, so the notice is worded
    /// differently and no one is listed as missing.
    pub in_progress: bool,
}

/// Everything the payroll report PDF renders. Assembled by
/// `services::payroll_report`; this module only lays it out.
pub struct PayrollReportData {
    /// Human-readable month, e.g. "May 2026".
    pub period_label: String,
    pub organization_name: String,
    /// `None` when no absence categories are selected for the report.
    pub absence_rows: Option<Vec<PayrollAbsenceRow>>,
    /// Empty when neither hours section is enabled.
    pub hours_sections: Vec<PayrollHoursSection>,
    /// Signed changes to days from already-reported months. Empty on almost
    /// every report, and the section is then left out entirely rather than
    /// printed as an empty table.
    pub late_entry_rows: Vec<PayrollLateEntryRow>,
    /// Signed per-person-day hour values this exact document declared. Regular
    /// working days carry their net totals; catch-up rows carry their positive
    /// or negative correction. Kept with the document so its ledger record
    /// cannot drift from what payroll received.
    pub declared_work_days: Vec<PayrollDeclaredWorkDay>,
    /// Exact older person-days printed in the corrections section, including
    /// legacy days whose unknown baseline must not be added to the new ledger.
    pub carried_work_days: Vec<PayrollCarriedWorkDay>,
    /// Database ids and row versions of regular absences accounted for here.
    pub reported_absence_snapshots: Vec<(i64, String)>,
    /// Absence periods from earlier months that no report ever showed, under
    /// their own real dates. Same "only when non-empty" rule as above.
    pub late_absence_rows: Vec<PayrollAbsenceRow>,
    /// Database ids and row versions of the absences `late_absence_rows` was built from, not
    /// rendered, and deliberately part of the assembled document rather than
    /// recomputed by the sender.
    ///
    /// The sender marks only these unchanged rows as declared.
    pub late_absence_snapshots: Vec<(i64, String)>,
    /// Cancellations of earlier reported absences requiring payroll review.
    pub cancelled_absence_rows: Vec<PayrollAbsenceRow>,
    /// Absences whose earlier reported weekly quota needs manual review.
    pub absence_review_rows: Vec<PayrollAbsenceRow>,
    /// The day the document was assembled. Printed under the title and used in
    /// the attachment's filename: the same month can be sent more than once
    /// (an interim snapshot, then the final report), so the recipient needs to
    /// tell the copies apart and know how current the figures are.
    pub created_on: NaiveDate,
    /// `Some` whenever the report is not a complete month: a manually
    /// triggered partial send, or an interim snapshot of the running month
    /// (`in_progress`). A complete report carries no notice.
    pub provisional: Option<ProvisionalNotice>,
}

/// Render the payroll report as PDF bytes.
pub fn render_payroll_report_pdf(data: &PayrollReportData, language: &Language) -> Vec<u8> {
    let mut renderer = Renderer::new(language, ABSENCE_COLUMNS);

    let title = i18n::translate(language, "pdf_payroll_title", &[]);
    let subtitle = if data.organization_name.trim().is_empty() {
        data.period_label.clone()
    } else {
        format!("{} - {}", data.organization_name.trim(), data.period_label)
    };
    renderer.draw_title_block(&title, &subtitle);
    renderer.draw_note(&i18n::translate(
        language,
        "pdf_payroll_created_on",
        &[("date", i18n::format_date(language, data.created_on))],
    ));

    if let Some(notice) = &data.provisional {
        render_provisional_notice(&mut renderer, language, notice);
    }

    if let Some(rows) = &data.absence_rows {
        render_absence_table(&mut renderer, language, rows);
    }
    for section in &data.hours_sections {
        render_hours_table(&mut renderer, language, section);
    }
    if !data.late_absence_rows.is_empty() {
        render_late_absence_table(&mut renderer, language, &data.late_absence_rows);
    }
    if !data.cancelled_absence_rows.is_empty() {
        render_absence_review_table(
            &mut renderer,
            language,
            &data.cancelled_absence_rows,
            "pdf_payroll_cancelled_absences_heading",
            "pdf_payroll_cancelled_absences_note",
            false,
        );
    }
    if !data.absence_review_rows.is_empty() {
        render_absence_review_table(
            &mut renderer,
            language,
            &data.absence_review_rows,
            "pdf_payroll_absence_review_heading",
            "pdf_payroll_absence_review_note",
            true,
        );
    }
    let late_entry_months = sum_late_entries_per_month(&data.late_entry_rows);
    if !late_entry_months.is_empty() {
        render_late_entry_table(&mut renderer, language, &late_entry_months);
    }

    super::build_pdf(renderer.finish())
}

/// Draw the "this report is not complete yet" block directly under the title,
/// listing who is still missing and why, so the recipient can see at a glance
/// that the figures below cover only part of the staff.
fn render_provisional_notice(
    renderer: &mut Renderer,
    language: &Language,
    notice: &ProvisionalNotice,
) {
    renderer.draw_section_heading(&i18n::translate(
        language,
        if notice.in_progress {
            "pdf_payroll_snapshot_heading"
        } else {
            "pdf_payroll_provisional_heading"
        },
        &[],
    ));
    if notice.in_progress {
        renderer.draw_note(&i18n::translate(
            language,
            "pdf_payroll_snapshot_summary",
            &[("included", notice.included.to_string())],
        ));
        return;
    }
    renderer.draw_note(&i18n::translate(
        language,
        "pdf_payroll_provisional_summary",
        &[
            ("included", notice.included.to_string()),
            ("total", notice.total.to_string()),
        ],
    ));
    if !notice.omitted.is_empty() {
        renderer.draw_note(&i18n::translate(
            language,
            "pdf_payroll_provisional_missing",
            &[],
        ));
        for person in &notice.omitted {
            renderer.draw_note(&format!(
                "- {} ({})",
                person.name,
                i18n::translate(language, person.reason_key, &[])
            ));
        }
    }
}

fn render_absence_table(renderer: &mut Renderer, language: &Language, rows: &[PayrollAbsenceRow]) {
    renderer.set_columns(ABSENCE_COLUMNS);
    let heading = i18n::translate(language, "pdf_payroll_absences_heading", &[]);
    renderer.draw_section_heading(&heading);

    if rows.is_empty() {
        renderer.draw_note(&i18n::translate(language, "pdf_payroll_no_rows", &[]));
        return;
    }

    renderer.draw_table_header();
    for (index, row) in rows.iter().enumerate() {
        renderer.draw_row(
            &[
                (0, row.employee.clone()),
                (1, row.category.clone()),
                (2, i18n::format_date(language, row.from)),
                (3, i18n::format_date(language, row.to)),
                (ABSENCE_DAYS_COLUMN, format_days(row.days, language)),
                (
                    ABSENCE_MEDICAL_CERTIFICATE_COLUMN,
                    format_medical_certificate_required(row.medical_certificate_required, language),
                ),
            ],
            index % 2 == 1,
        );
    }
}

/// "AU" column text: localized Yes/No when the category tracks it, or a dash
/// when it doesn't apply to this category at all.
fn format_medical_certificate_required(required: Option<bool>, language: &Language) -> String {
    match required {
        Some(true) => i18n::translate(language, "pdf_payroll_medical_certificate_yes", &[]),
        Some(false) => i18n::translate(language, "pdf_payroll_medical_certificate_no", &[]),
        None => "\u{2013}".to_string(),
    }
}

fn render_hours_table(renderer: &mut Renderer, language: &Language, section: &PayrollHoursSection) {
    renderer.set_columns(HOURS_COLUMNS);
    let heading = i18n::translate(language, section.heading_key, &[]);
    renderer.draw_section_heading(&heading);

    if section.rows.is_empty() {
        renderer.draw_note(&i18n::translate(language, "pdf_payroll_no_rows", &[]));
        return;
    }

    renderer.draw_table_header();
    for (index, row) in section.rows.iter().enumerate() {
        renderer.draw_row(
            &[
                (0, row.employee.clone()),
                (HOURS_WORK_DAYS_COLUMN, row.work_days.to_string()),
                (HOURS_HOURS_COLUMN, format_minutes(row.minutes)),
                (
                    HOURS_DECIMAL_COLUMN,
                    format_decimal_hours(row.minutes, language),
                ),
            ],
            index % 2 == 1,
        );
    }
}

/// Draw the "reported after the fact" absence table.
///
/// Same shape as the ordinary absence table, but the dates are the absence's
/// real ones rather than a clamp to the reported month — payroll has to book
/// continued pay against the month it was actually taken in.
fn render_late_absence_table(
    renderer: &mut Renderer,
    language: &Language,
    rows: &[PayrollAbsenceRow],
) {
    renderer.set_columns(ABSENCE_COLUMNS);
    renderer.draw_section_heading(&i18n::translate(
        language,
        "pdf_payroll_late_absences_heading",
        &[],
    ));
    renderer.draw_note(&i18n::translate(
        language,
        "pdf_payroll_late_absences_note",
        &[],
    ));

    renderer.draw_table_header();
    for (index, row) in rows.iter().enumerate() {
        renderer.draw_row(
            &[
                (0, row.employee.clone()),
                (1, row.category.clone()),
                (2, i18n::format_date(language, row.from)),
                (3, i18n::format_date(language, row.to)),
                (ABSENCE_DAYS_COLUMN, format_days(row.days, language)),
                (
                    ABSENCE_MEDICAL_CERTIFICATE_COLUMN,
                    format_medical_certificate_required(row.medical_certificate_required, language),
                ),
            ],
            index % 2 == 1,
        );
    }
}

/// List cancelled absences without guessing a historical day count.
fn render_absence_review_table(
    renderer: &mut Renderer,
    language: &Language,
    rows: &[PayrollAbsenceRow],
    heading: &str,
    note: &str,
    show_certificate: bool,
) {
    renderer.set_columns(if show_certificate {
        REVIEW_ABSENCE_COLUMNS
    } else {
        CANCELLED_ABSENCE_COLUMNS
    });
    renderer.draw_section_heading(&i18n::translate(language, heading, &[]));
    renderer.draw_note(&i18n::translate(language, note, &[]));
    renderer.draw_table_header();
    for (index, row) in rows.iter().enumerate() {
        let mut cells = vec![
            (0, row.employee.clone()),
            (1, row.category.clone()),
            (2, i18n::format_date(language, row.from)),
            (3, i18n::format_date(language, row.to)),
        ];
        if show_certificate {
            cells.push((
                4,
                format_medical_certificate_required(row.medical_certificate_required, language),
            ));
        }
        renderer.draw_row(&cells, index % 2 == 1);
    }
}

/// The people the corrections table prints a line for. Somebody whose changes
/// all cancel out within their months has none, and a document with nothing
/// else in it is not worth sending.
pub fn people_with_corrections(rows: &[PayrollLateEntryRow]) -> impl Iterator<Item = &str> {
    sum_late_entries_per_month(rows)
        .into_iter()
        .map(|month| month.employee)
}

/// Draw the corrections table, with a line of explanation above it.
///
/// The note matters more than the table: without it the payroll accountant sees
/// dates from a month they have already filed and has no way to know whether
/// this is a correction, a duplicate, or something they were never sent.
fn render_late_entry_table(
    renderer: &mut Renderer,
    language: &Language,
    months: &[LateEntryMonth],
) {
    renderer.set_columns(LATE_ENTRY_COLUMNS);
    renderer.draw_section_heading(&i18n::translate(
        language,
        "pdf_payroll_late_entries_heading",
        &[],
    ));
    renderer.draw_note(&i18n::translate(
        language,
        "pdf_payroll_late_entries_note",
        &[],
    ));

    renderer.draw_table_header();
    for (index, month) in months.iter().enumerate() {
        renderer.draw_row(
            &[
                (0, month.employee.to_string()),
                (
                    LATE_ENTRY_MONTH_COLUMN,
                    i18n::format_month(language, month.year, month.month),
                ),
                (
                    LATE_ENTRY_HOURS_COLUMN,
                    format_signed_minutes(month.minutes),
                ),
                (
                    LATE_ENTRY_DECIMAL_COLUMN,
                    format_signed_decimal_hours(month.minutes, language),
                ),
                (LATE_ENTRY_TOTAL_COLUMN, format_minutes(month.total_minutes)),
            ],
            index % 2 == 1,
        );
    }
}

/// Decimal separator of the report language — payroll software and accountants
/// read these numbers directly, so a German report must print `7,50`.
fn decimal_separator(language: &Language) -> char {
    if language.code() == "de" {
        ','
    } else {
        '.'
    }
}

/// Hours as a decimal number with two digits, e.g. `7.50` / `7,50`. Payroll is
/// calculated in decimal hours, so the report carries both notations.
fn format_decimal_hours(total_minutes: i64, language: &Language) -> String {
    let hours = total_minutes as f64 / 60.0;
    format!("{hours:.2}").replace('.', &decimal_separator(language).to_string())
}

fn format_signed_decimal_hours(total_minutes: i64, language: &Language) -> String {
    let formatted = format_decimal_hours(total_minutes, language);
    if total_minutes >= 0 {
        format!("+{formatted}")
    } else {
        formatted
    }
}

/// Absence days without trailing noise: whole days print as `3`, the rare
/// fractional value as `3,5`.
fn format_days(days: f64, language: &Language) -> String {
    if (days - days.round()).abs() < 0.001 {
        return format!("{}", days.round() as i64);
    }
    format!("{days:.1}").replace('.', &decimal_separator(language).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    fn late_row(
        user_id: i64,
        day: NaiveDate,
        minutes: i64,
        month_total_minutes: i64,
    ) -> PayrollLateEntryRow {
        PayrollLateEntryRow {
            user_id,
            employee: format!("User {user_id}"),
            date: day,
            minutes,
            month_total_minutes,
        }
    }

    fn month_summary(rows: &[PayrollLateEntryRow]) -> Vec<(String, i32, u32, i64, i64)> {
        sum_late_entries_per_month(rows)
            .into_iter()
            .map(|m| (m.employee.to_string(), m.year, m.month, m.minutes, m.total_minutes))
            .collect()
    }

    #[test]
    fn late_entries_are_summed_per_person_and_month() {
        // 20 hours were booked before; +20 hours arrive for the same month.
        let rows = [
            late_row(1, date(2026, 3, 9), 600, 2400),
            late_row(1, date(2026, 3, 10), -30, 2400),
            late_row(1, date(2026, 3, 20), 630, 2400),
            late_row(1, date(2026, 4, 2), 60, 900),
            late_row(2, date(2026, 3, 5), 20, 500),
        ];
        assert_eq!(
            month_summary(&rows),
            vec![
                ("User 1".to_string(), 2026, 3, 1200, 2400),
                ("User 1".to_string(), 2026, 4, 60, 900),
                ("User 2".to_string(), 2026, 3, 20, 500),
            ]
        );
    }

    #[test]
    fn late_entries_are_ordered_by_name_before_month() {
        let mut anna = late_row(2, date(2026, 3, 10), 30, 100);
        anna.employee = "Anna".to_string();
        let mut berta_march = late_row(1, date(2026, 3, 9), 60, 200);
        berta_march.employee = "Berta".to_string();
        let mut berta_february = late_row(1, date(2026, 2, 9), 45, 300);
        berta_february.employee = "Berta".to_string();
        assert_eq!(
            month_summary(&[berta_march, anna, berta_february]),
            vec![
                ("Anna".to_string(), 2026, 3, 30, 100),
                ("Berta".to_string(), 2026, 2, 45, 300),
                ("Berta".to_string(), 2026, 3, 60, 200),
            ]
        );
    }

    #[test]
    fn late_entries_cancelling_out_within_a_month_are_dropped() {
        // A shift moved from the 10th to the 9th changes nothing for the month.
        let rows = [
            late_row(1, date(2026, 3, 9), 120, 480),
            late_row(1, date(2026, 3, 10), -120, 480),
            late_row(1, date(2026, 4, 1), 30, 90),
        ];
        assert_eq!(
            month_summary(&rows),
            vec![("User 1".to_string(), 2026, 4, 30, 90)]
        );
    }

    #[test]
    fn renders_a_pdf_with_all_sections() {
        let language = Language::default();
        let data = PayrollReportData {
            period_label: "May 2026".into(),
            organization_name: "Example GmbH".into(),
            absence_rows: Some(vec![PayrollAbsenceRow {
                user_id: 1,
                employee: "Smith, John".into(),
                category: "Sick".into(),
                from: date(2026, 5, 4),
                to: date(2026, 5, 6),
                days: 3.0,
                medical_certificate_required: Some(false),
            }]),
            hours_sections: vec![PayrollHoursSection {
                heading_key: "pdf_payroll_assistant_hours_heading",
                rows: vec![PayrollHoursRow {
                    user_id: 2,
                    employee: "Doe, Jane".into(),
                    work_days: 4,
                    minutes: 930,
                }],
            }],
            // A day from April that only reached the system in May, so the
            // catch-up table is laid out at least once here.
            late_entry_rows: vec![PayrollLateEntryRow {
                user_id: 2,
                employee: "Doe, Jane".into(),
                date: date(2026, 4, 28),
                minutes: 240,
                month_total_minutes: 1500,
            }],
            declared_work_days: Vec::new(),
            carried_work_days: Vec::new(),
            reported_absence_snapshots: Vec::new(),
            late_absence_rows: Vec::new(),
            late_absence_snapshots: Vec::new(),
            cancelled_absence_rows: vec![PayrollAbsenceRow {
                user_id: 1,
                employee: "Smith, John".into(),
                category: "Sick".into(),
                from: date(2026, 4, 1),
                to: date(2026, 4, 2),
                days: 0.0,
                medical_certificate_required: None,
            }],
            absence_review_rows: vec![PayrollAbsenceRow {
                user_id: 1,
                employee: "Smith, John".into(),
                category: "Sick".into(),
                from: date(2026, 4, 3),
                to: date(2026, 4, 4),
                days: 0.0,
                medical_certificate_required: Some(true),
            }],
            created_on: NaiveDate::from_ymd_opt(2026, 9, 2).unwrap(),
            provisional: None,
        };
        let bytes = render_payroll_report_pdf(&data, &language);
        assert!(bytes.starts_with(b"%PDF"));
        let pdf = String::from_utf8_lossy(&bytes);
        assert!(pdf.contains("Cancelled absences"));
        assert!(pdf.contains("Absences - review days"));
        assert!(pdf.contains("Yes"));
    }

    /// A provisional report must render in every language — the notice goes
    /// through `i18n::translate`, which panics on a missing key/parameter.
    #[test]
    fn renders_a_provisional_pdf_in_every_language() {
        for code in ["en", "de"] {
            let language = Language::from_setting(code);
            let data = PayrollReportData {
                period_label: "May 2026".into(),
                organization_name: "Example GmbH".into(),
                absence_rows: Some(vec![]),
                hours_sections: vec![],
                late_entry_rows: Vec::new(),
                declared_work_days: Vec::new(),
                carried_work_days: Vec::new(),
                reported_absence_snapshots: Vec::new(),
                late_absence_rows: Vec::new(),
                late_absence_snapshots: Vec::new(),
                cancelled_absence_rows: vec![PayrollAbsenceRow {
                    user_id: 1,
                    employee: "Smith, John".into(),
                    category: "Sick".into(),
                    from: date(2026, 4, 1),
                    to: date(2026, 4, 2),
                    days: 0.0,
                    medical_certificate_required: None,
                }],
                absence_review_rows: vec![PayrollAbsenceRow {
                    user_id: 1,
                    employee: "Smith, John".into(),
                    category: "Sick".into(),
                    from: date(2026, 4, 3),
                    to: date(2026, 4, 4),
                    days: 0.0,
                    medical_certificate_required: None,
                }],
                created_on: NaiveDate::from_ymd_opt(2026, 9, 2).unwrap(),
                provisional: Some(ProvisionalNotice {
                    included: 8,
                    total: 12,
                    omitted: vec![PayrollOmittedPerson {
                        name: "Doe, Jane".into(),
                        reason_key: "payroll_report_reason_not_submitted",
                    }],
                    in_progress: false,
                }),
            };
            let bytes = render_payroll_report_pdf(&data, &language);
            assert!(bytes.starts_with(b"%PDF"), "{code}");
        }
    }

    /// The interim-snapshot notice takes a different wording branch with its
    /// own translation keys, so it needs the same every-language guard.
    #[test]
    fn renders_an_interim_snapshot_pdf_in_every_language() {
        for code in ["en", "de"] {
            let language = Language::from_setting(code);
            let data = PayrollReportData {
                period_label: "August 2026".into(),
                organization_name: "Example GmbH".into(),
                absence_rows: Some(vec![]),
                hours_sections: vec![],
                late_entry_rows: Vec::new(),
                declared_work_days: Vec::new(),
                carried_work_days: Vec::new(),
                reported_absence_snapshots: Vec::new(),
                late_absence_rows: Vec::new(),
                late_absence_snapshots: Vec::new(),
                cancelled_absence_rows: Vec::new(),
                absence_review_rows: Vec::new(),
                created_on: NaiveDate::from_ymd_opt(2026, 9, 2).unwrap(),
                provisional: Some(ProvisionalNotice {
                    included: 5,
                    total: 5,
                    omitted: vec![],
                    in_progress: true,
                }),
            };
            let bytes = render_payroll_report_pdf(&data, &language);
            assert!(bytes.starts_with(b"%PDF"), "{code}");
        }
    }

    #[test]
    fn renders_a_pdf_when_every_section_is_empty() {
        let language = Language::default();
        let data = PayrollReportData {
            period_label: "May 2026".into(),
            organization_name: String::new(),
            absence_rows: Some(vec![]),
            hours_sections: vec![PayrollHoursSection {
                heading_key: "pdf_payroll_assistant_hours_heading",
                rows: vec![],
            }],
            late_entry_rows: Vec::new(),
            declared_work_days: Vec::new(),
            carried_work_days: Vec::new(),
            reported_absence_snapshots: Vec::new(),
            late_absence_rows: Vec::new(),
            late_absence_snapshots: Vec::new(),
            cancelled_absence_rows: Vec::new(),
            absence_review_rows: Vec::new(),
            created_on: NaiveDate::from_ymd_opt(2026, 9, 2).unwrap(),
            provisional: None,
        };
        let bytes = render_payroll_report_pdf(&data, &language);
        assert!(bytes.starts_with(b"%PDF"));
    }
}
