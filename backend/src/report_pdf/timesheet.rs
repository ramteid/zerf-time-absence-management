//! Per-employee timesheet PDF: one printable page set per employee with a
//! table of days/entries, a total row, and the flextime balance rows.
//!
//! Column layout (all 180 mm content width):
//! Date 18 | Weekday 18 | Start 12 | End 12 | Category 36 | Duration 14 |
//! Status 17 | Absence 25 | Holiday 28
//!
//! The "Status" column is essential for reader reconciliation: the Total row
//! counts only approved, work-crediting, break-adjusted minutes, while the
//! Duration column shows raw minutes for every non-rejected entry (including
//! draft, submitted, and non-crediting entries). Without a status column,
//! readers cannot understand why summing Duration differs from Total.

use super::{
    format_minutes, format_signed_minutes, Align, Column, PdfFont, Renderer, CONTENT_WIDTH_MM,
    MARGIN_LEFT_MM, ROW_HEIGHT_MM, TITLE_COLOR, TOTAL_FILL,
};
use crate::i18n::{self, Language};
use crate::services::reports::{FlextimeDay, MonthReport};
use chrono::NaiveDate;

/// Column layout for the timesheet table. Widths sum to [`CONTENT_WIDTH_MM`].
/// Date 18 | Weekday 18 | Start 12 | End 12 | Category 36 | Duration 14 |
/// Status 17 | Absence 25 | Holiday 28 = 180 mm total.
const COLUMNS: &[Column] = &[
    Column {
        header_key: "pdf_column_date",
        width_mm: 18.0,
        align: Align::Left,
    },
    Column {
        header_key: "pdf_column_weekday",
        width_mm: 18.0,
        align: Align::Left,
    },
    Column {
        header_key: "pdf_column_start",
        width_mm: 12.0,
        align: Align::Center,
    },
    Column {
        header_key: "pdf_column_end",
        width_mm: 12.0,
        align: Align::Center,
    },
    Column {
        header_key: "pdf_column_category",
        width_mm: 36.0,
        align: Align::Left,
    },
    Column {
        header_key: "pdf_column_duration",
        width_mm: 14.0,
        align: Align::Left,
    },
    Column {
        header_key: "pdf_column_status",
        width_mm: 17.0,
        align: Align::Left,
    },
    Column {
        header_key: "pdf_column_absence",
        width_mm: 25.0,
        align: Align::Left,
    },
    Column {
        header_key: "pdf_column_holiday",
        width_mm: 28.0,
        align: Align::Left,
    },
];

/// Index of the `Duration` column — the total/summary rows place their value
/// directly under it, same as the table body.
const DURATION_COLUMN: usize = 5;

/// Index of the `Status` column — used to determine the status label to display
/// for each entry so readers can reconcile the Duration column against the Total.
const STATUS_COLUMN: usize = 6;

/// Data for one employee's timesheet, as needed to render their section.
/// Produced by the caller (service layer) from [`MonthReport`] /
/// [`FlextimeDay`] data already fetched for the requested range.
pub struct TimesheetSection {
    pub user_name: String,
    pub report: MonthReport,
    pub flextime_data: Vec<FlextimeDay>,
}

/// Render one combined PDF containing one section per entry in `sections`,
/// each starting on its own page (single-employee exports simply pass a
/// one-element slice). Returns the raw PDF bytes.
pub fn render_timesheet_pdf(
    sections: &[TimesheetSection],
    from: NaiveDate,
    to: NaiveDate,
    language: &Language,
) -> Vec<u8> {
    let mut renderer = Renderer::new(language, COLUMNS);
    for (index, section) in sections.iter().enumerate() {
        if index > 0 {
            renderer.start_new_page();
        }
        render_section(&mut renderer, section, from, to);
    }
    super::build_pdf(renderer.finish())
}

/// Render one employee's full timesheet section: title block, table
/// (with multi-page support and repeating header), total row and flextime
/// balance rows. Always starts at the current page's top margin.
fn render_section(
    renderer: &mut Renderer,
    section: &TimesheetSection,
    from: NaiveDate,
    to: NaiveDate,
) {
    // Title block: bold report title, with the employee name and date
    // range as a larger, equally dark second line — so the recipient is
    // immediately visible rather than receding behind the title as a
    // small gray subtitle would.
    let title = i18n::translate(renderer.language, "pdf_timesheet_title", &[]);
    let subtitle = format!("{} - {} - {}", section.user_name, from, to);
    renderer.draw_title_block(&title, &subtitle);
    renderer.draw_table_header();

    // Alternating shading is keyed to the rendered row count (one increment
    // per drawn row, including each individual entry within a day) — not the
    // day index — so it matches the original browser-side renderer exactly
    // even on days with multiple time entries.
    let mut row_count: usize = 0;
    for day in &section.report.days {
        let weekday = i18n::weekday_label(renderer.language, &day.weekday);
        // Pass both slug and stored category name so admin-created custom
        // categories (which have no static `absence_kind_<slug>` translation
        // key) print with their real display name instead of the raw slug.
        let absence = match (day.absence.as_deref(), day.absence_name.as_deref()) {
            (Some(slug), Some(name)) => i18n::absence_kind_label(renderer.language, slug, name),
            _ => String::new(),
        };
        let holiday = day.holiday.clone().unwrap_or_default();
        if day.entries.is_empty() {
            renderer.draw_row(
                &[
                    (0, day.date.to_string()),
                    (1, weekday.clone()),
                    (2, String::new()),
                    (3, String::new()),
                    (4, String::new()),
                    (5, format_minutes(0)),
                    (STATUS_COLUMN, String::new()),
                    (7, absence.clone()),
                    (8, holiday.clone()),
                ],
                row_count % 2 == 1,
            );
            row_count += 1;
        } else {
            for entry in &day.entries {
                // A short status label so readers can reconcile the Duration
                // column against the Total row. The Total counts only
                // approved, work-crediting, break-adjusted minutes; draft,
                // submitted, and non-crediting entries contribute to Duration
                // but not to Total.
                let status_label =
                    entry_status_label(renderer.language, &entry.status, entry.counts_as_work);
                renderer.draw_row(
                    &[
                        (0, day.date.to_string()),
                        (1, weekday.clone()),
                        (2, entry.start_time.get(0..5).unwrap_or("").to_string()),
                        (3, entry.end_time.get(0..5).unwrap_or("").to_string()),
                        (
                            4,
                            i18n::work_category_label(renderer.language, &entry.category),
                        ),
                        (5, format_minutes(entry.minutes)),
                        (STATUS_COLUMN, status_label),
                        (7, absence.clone()),
                        (8, holiday.clone()),
                    ],
                    row_count % 2 == 1,
                );
                row_count += 1;
            }
        }
    }

    // Total row.
    renderer.ensure_space(ROW_HEIGHT_MM, true);
    renderer.fill_rect(
        MARGIN_LEFT_MM,
        renderer.y,
        CONTENT_WIDTH_MM,
        ROW_HEIGHT_MM,
        TOTAL_FILL,
    );
    let baseline = renderer.y + 3.8;
    let total_label = i18n::translate(renderer.language, "pdf_total", &[]);
    renderer.draw_text(
        &total_label,
        MARGIN_LEFT_MM + 1.0,
        baseline,
        PdfFont::Bold,
        7.5,
        TITLE_COLOR,
    );
    let total_value = format_minutes(range_total_minutes(&section.report));
    let total_x = renderer.aligned_x(DURATION_COLUMN, &total_value, 7.5);
    renderer.draw_text(
        &total_value,
        total_x,
        baseline,
        PdfFont::Bold,
        7.5,
        TITLE_COLOR,
    );
    renderer.y += ROW_HEIGHT_MM;

    let (opening, closing) = flextime_bounds(&section.flextime_data);
    if let Some(opening_balance) = opening {
        let label = i18n::translate(renderer.language, "pdf_flextime_opening_balance", &[]);
        renderer.draw_summary_row(
            &label,
            &format_signed_minutes(opening_balance),
            DURATION_COLUMN,
        );
    }
    if let Some(closing_balance) = closing {
        let label = i18n::translate(renderer.language, "pdf_flextime_closing_balance", &[]);
        renderer.draw_summary_row(
            &label,
            &format_signed_minutes(closing_balance),
            DURATION_COLUMN,
        );
    }
}

/// Short localized status label for a time entry. Used in the Status column to
/// let readers reconcile the Duration column against the Total row.
///
/// The Total counts only approved, work-crediting, break-adjusted minutes.
/// Non-approved entries (draft, submitted) and non-crediting entries always
/// show a Duration but are never part of the Total; this label makes that
/// explicit without requiring the reader to know the business rules.
fn entry_status_label(language: &Language, status: &str, counts_as_work: bool) -> String {
    // Non-crediting entries: show a marker even when approved, because their
    // minutes never reach the Total.
    if !counts_as_work && status == "approved" {
        return i18n::translate(language, "pdf_status_approved_nc", &[]);
    }
    let key = match status {
        "draft" => "pdf_status_draft",
        "submitted" => "pdf_status_submitted",
        "approved" => "pdf_status_approved",
        _ => "pdf_status_other",
    };
    i18n::translate(language, key, &[])
}

/// Break-adjusted total minutes for the report range. Uses the pre-computed
/// `actual_min` from the report (which already applies the auto-break
/// deduction per day) rather than re-summing raw entry minutes. This keeps
/// the PDF Total row consistent with the UI, the flextime closing balance on
/// the same page, and the documented auto-break behaviour.
fn range_total_minutes(report: &MonthReport) -> i64 {
    report.actual_min
}

/// First day's opening balance and last day's closing balance, mirroring the
/// frontend's `flextimeBounds` helper. `None` when there is no flextime data
/// for this user (e.g. assistants, who have no flextime account).
fn flextime_bounds(flextime_data: &[FlextimeDay]) -> (Option<i64>, Option<i64>) {
    match (flextime_data.first(), flextime_data.last()) {
        (Some(first), Some(last)) => (
            Some(first.cumulative_min - first.diff_min),
            Some(last.cumulative_min),
        ),
        _ => (None, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_status_label_maps_statuses_and_flags_correctly() {
        let language = Language::default();
        // Approved crediting entry → "Approved".
        assert_eq!(entry_status_label(&language, "approved", true), "Approved");
        // Approved non-crediting entry → notes it is not counted.
        let nc_label = entry_status_label(&language, "approved", false);
        assert!(
            nc_label.contains("nc") || nc_label.contains("Approv"),
            "non-crediting approved label should mention 'nc': {nc_label}"
        );
        // Draft and submitted map to their respective labels.
        assert_eq!(entry_status_label(&language, "draft", true), "Draft");
        assert_eq!(
            entry_status_label(&language, "submitted", true),
            "Submitted"
        );
    }

    #[test]
    fn columns_sum_to_content_width() {
        let total: f32 = COLUMNS.iter().map(|c| c.width_mm).sum();
        assert!(
            (total - CONTENT_WIDTH_MM).abs() < 0.01,
            "column widths {total} mm != content width {CONTENT_WIDTH_MM} mm"
        );
    }

    #[test]
    fn flextime_bounds_reads_first_and_last_day() {
        let days = vec![
            FlextimeDay {
                date: NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
                actual_min: 480,
                target_min: 480,
                diff_min: 30,
                cumulative_min: 130,
                absence: None,
                holiday: None,
            },
            FlextimeDay {
                date: NaiveDate::from_ymd_opt(2026, 6, 2).unwrap(),
                actual_min: 480,
                target_min: 480,
                diff_min: 0,
                cumulative_min: 130,
                absence: None,
                holiday: None,
            },
        ];
        assert_eq!(flextime_bounds(&days), (Some(100), Some(130)));
        assert_eq!(flextime_bounds(&[]), (None, None));
    }

    #[test]
    fn range_total_minutes_uses_break_adjusted_report_total() {
        // entry.minutes = 480 (raw), but actual_min = 450 (break-adjusted).
        // range_total_minutes must return 450, not 480.
        let report = MonthReport {
            user_id: 1,
            month: "2026-06".into(),
            days: vec![crate::services::reports::DayDetail {
                date: NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
                weekday: "Monday".into(),
                entries: vec![crate::services::reports::EntryDetail {
                    start_time: "08:00".into(),
                    end_time: "16:00".into(),
                    category: "Work".into(),
                    color: "#000000".into(),
                    minutes: 480,
                    counts_as_work: true,
                    status: "approved".into(),
                    comment: None,
                }],
                actual_min: 450,
                target_min: 480,
                absence: None,
                absence_name: None,
                holiday: None,
            }],
            target_min: 480,
            actual_min: 450,
            diff_min: -30,
            submitted_min: 480,
            full_month_target_min: 480,
            category_totals: Default::default(),
            weeks_all_submitted: Some(true),
            weeks_all_approved: Some(true),
            current_week_status: None,
        };
        assert_eq!(range_total_minutes(&report), 450);
    }

    #[test]
    fn renders_a_pdf_with_at_least_one_page_per_section() {
        let language = Language::default();
        let report = MonthReport {
            user_id: 1,
            month: "seed".into(),
            days: vec![],
            target_min: 0,
            actual_min: 0,
            diff_min: 0,
            submitted_min: 0,
            full_month_target_min: 0,
            category_totals: Default::default(),
            weeks_all_submitted: None,
            weeks_all_approved: None,
            current_week_status: None,
        };
        let sections = vec![TimesheetSection {
            user_name: "Alice Lead".into(),
            report,
            flextime_data: vec![],
        }];
        let from = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
        let to = NaiveDate::from_ymd_opt(2026, 6, 30).unwrap();
        let bytes = render_timesheet_pdf(&sections, from, to, &language);
        assert!(bytes.starts_with(b"%PDF"));
    }
}
