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

use super::{format_minutes, Align, Column, Renderer};
use crate::i18n::{self, Language};
use chrono::NaiveDate;

/// Absence table: one row per absence period, clamped to the reported month.
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
        align: Align::Right,
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

/// One absence period of one employee within the reported month.
pub struct PayrollAbsenceRow {
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
    /// `Some` only for a manually triggered partial send; a complete report
    /// carries no notice.
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

    if let Some(notice) = &data.provisional {
        render_provisional_notice(&mut renderer, language, notice);
    }

    if let Some(rows) = &data.absence_rows {
        render_absence_table(&mut renderer, language, rows);
    }
    for section in &data.hours_sections {
        render_hours_table(&mut renderer, language, section);
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

    let total_days: i64 = section.rows.iter().map(|row| row.work_days).sum();
    let total_minutes: i64 = section.rows.iter().map(|row| row.minutes).sum();
    renderer.draw_total_row(
        &i18n::translate(language, "pdf_payroll_total", &[]),
        &[
            (HOURS_WORK_DAYS_COLUMN, total_days.to_string()),
            (HOURS_HOURS_COLUMN, format_minutes(total_minutes)),
            (
                HOURS_DECIMAL_COLUMN,
                format_decimal_hours(total_minutes, language),
            ),
        ],
    );
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

    #[test]
    fn columns_sum_to_content_width() {
        for (name, columns) in [("absence", ABSENCE_COLUMNS), ("hours", HOURS_COLUMNS)] {
            let total: f32 = columns.iter().map(|column| column.width_mm).sum();
            assert!(
                (total - super::super::CONTENT_WIDTH_MM).abs() < 0.01,
                "{name} column widths {total} mm != content width"
            );
        }
    }

    #[test]
    fn decimal_hours_use_the_language_separator() {
        let english = Language::from_setting("en");
        let german = Language::from_setting("de");
        assert_eq!(format_decimal_hours(450, &english), "7.50");
        assert_eq!(format_decimal_hours(450, &german), "7,50");
        assert_eq!(format_decimal_hours(0, &english), "0.00");
    }

    #[test]
    fn days_print_without_decimals_unless_fractional() {
        let german = Language::from_setting("de");
        assert_eq!(format_days(3.0, &german), "3");
        assert_eq!(format_days(0.0, &german), "0");
        assert_eq!(format_days(3.5, &german), "3,5");
    }

    #[test]
    fn renders_a_pdf_with_all_sections() {
        let language = Language::default();
        let data = PayrollReportData {
            period_label: "May 2026".into(),
            organization_name: "Example GmbH".into(),
            absence_rows: Some(vec![PayrollAbsenceRow {
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
                    employee: "Doe, Jane".into(),
                    work_days: 4,
                    minutes: 930,
                }],
            }],
            provisional: None,
        };
        let bytes = render_payroll_report_pdf(&data, &language);
        assert!(bytes.starts_with(b"%PDF"));
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
            provisional: None,
        };
        let bytes = render_payroll_report_pdf(&data, &language);
        assert!(bytes.starts_with(b"%PDF"));
    }
}
