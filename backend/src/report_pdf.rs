//! Server-side timesheet PDF rendering.
//!
//! Pure rendering logic with no database access (mirrors `email.rs` in that
//! respect). Lays out one printable timesheet per [`TimesheetSection`] — title,
//! a table of days/entries, a total row, and flextime balance rows — using
//! `pdf-writer`'s content-stream operators with the PDF standard Type 1 fonts
//! (Helvetica / Helvetica-Bold). Built-in fonts require no font files to
//! bundle, which keeps the binary small and avoids font-licensing questions.
//!
//! Coordinates are tracked in millimetres from the top-left corner (matching
//! how the layout was originally designed) and converted to PDF points
//! (1 pt = 1/72 in, bottom-left origin) only when emitting content-stream ops,
//! via [`mm_to_pt`] and [`Renderer::y_pt`].
//!
//! Right-/center-aligned columns contain only digits, `:`, `+`, `-`, `.` and
//! spaces, so [`glyph_width_1000`] hardcodes the public-domain Adobe AFM
//! Helvetica metrics for that subset, giving enough precision to compute
//! alignment offsets. Every other string is left-aligned, sidestepping the
//! missing-metrics problem for translated, variable-width text.

use crate::i18n::{self, Language};
use crate::services::reports::{FlextimeDay, MonthReport};
use chrono::NaiveDate;
use pdf_writer::{Content, Finish, Name, Pdf, Rect, Ref, Str};

// -- Page geometry (millimetres) -----------------------------------------------

const PAGE_WIDTH_MM: f32 = 210.0;
const PAGE_HEIGHT_MM: f32 = 297.0;
const MARGIN_LEFT_MM: f32 = 15.0;
const MARGIN_TOP_MM: f32 = 15.0;
const CONTENT_WIDTH_MM: f32 = 180.0;
const ROW_HEIGHT_MM: f32 = 5.5;
const HEADER_HEIGHT_MM: f32 = 7.0;
const PAGE_BOTTOM_MARGIN_MM: f32 = 15.0;

// -- Palette (matches the previous browser-rendered timesheet PDF) --------------

const TITLE_COLOR: (u8, u8, u8) = (20, 20, 20);
const HEADER_FILL: (u8, u8, u8) = (235, 235, 235);
const HEADER_TEXT: (u8, u8, u8) = (50, 50, 50);
const ROW_TEXT: (u8, u8, u8) = (30, 30, 30);
const ROW_SHADE_FILL: (u8, u8, u8) = (248, 248, 248);
const ROW_DIVIDER: (u8, u8, u8) = (220, 220, 220);
const SUMMARY_TEXT: (u8, u8, u8) = (90, 90, 90);
const TOTAL_FILL: (u8, u8, u8) = (235, 235, 235);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Align {
    Left,
    Center,
    Right,
}

/// One table column: its translated header key, width in millimetres, and how
/// its cell values should be aligned within that width.
struct Column {
    header_key: &'static str,
    width_mm: f32,
    align: Align,
}

/// Column layout for the timesheet table. Widths sum to [`CONTENT_WIDTH_MM`].
const COLUMNS: &[Column] = &[
    Column { header_key: "pdf_column_date",     width_mm: 22.0, align: Align::Left   },
    Column { header_key: "pdf_column_weekday",  width_mm: 20.0, align: Align::Left   },
    Column { header_key: "pdf_column_start",    width_mm: 12.0, align: Align::Center },
    Column { header_key: "pdf_column_end",      width_mm: 12.0, align: Align::Center },
    Column { header_key: "pdf_column_category", width_mm: 40.0, align: Align::Left   },
    Column { header_key: "pdf_column_duration", width_mm: 16.0, align: Align::Right  },
    Column { header_key: "pdf_column_absence",  width_mm: 25.0, align: Align::Left   },
    Column { header_key: "pdf_column_holiday",  width_mm: 33.0, align: Align::Left   },
];

/// Index of the `Duration` column — the total/summary rows place their value
/// directly under it, same as the table body.
const DURATION_COLUMN: usize = 5;

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
    let mut renderer = Renderer::new(language);
    for (index, section) in sections.iter().enumerate() {
        if index > 0 {
            renderer.start_new_page();
        }
        renderer.render_section(section, from, to);
    }
    build_pdf(renderer.finish())
}

// -- PDF document assembly -----------------------------------------------------

/// Assemble a multi-page PDF document from pre-rendered content streams.
///
/// Object ID allocation: 1 = catalog, 2 = page tree, 3 = Helvetica font,
/// 4 = Helvetica-Bold font, 5..5+n = page objects, 5+n..5+2n = content
/// streams (one per page).
fn build_pdf(page_streams: Vec<Vec<u8>>) -> Vec<u8> {
    let n = page_streams.len();

    let catalog_id   = Ref::new(1);
    let page_tree_id = Ref::new(2);
    let helv_id      = Ref::new(3);
    let helvb_id     = Ref::new(4);
    let page_ids: Vec<Ref>    = (0..n).map(|i| Ref::new(5 + i as i32)).collect();
    let content_ids: Vec<Ref> = (0..n).map(|i| Ref::new(5 + n as i32 + i as i32)).collect();

    let mut pdf = Pdf::new();

    pdf.catalog(catalog_id).pages(page_tree_id);
    pdf.pages(page_tree_id)
        .kids(page_ids.iter().copied())
        .count(n as i32);

    // Standard PDF Type 1 fonts — every conforming PDF reader includes them;
    // no font data needs to be embedded.
    pdf.type1_font(helv_id)
        .base_font(Name(b"Helvetica"))
        .encoding_predefined(Name(b"WinAnsiEncoding"));
    pdf.type1_font(helvb_id)
        .base_font(Name(b"Helvetica-Bold"))
        .encoding_predefined(Name(b"WinAnsiEncoding"));

    for i in 0..n {
        let mut page = pdf.page(page_ids[i]);
        page.parent(page_tree_id);
        page.media_box(Rect::new(
            0.0,
            0.0,
            mm_to_pt(PAGE_WIDTH_MM),
            mm_to_pt(PAGE_HEIGHT_MM),
        ));
        page.contents(content_ids[i]);
        {
            let mut res = page.resources();
            let mut fonts = res.fonts();
            fonts.pair(Name(b"Helv"),  helv_id);
            fonts.pair(Name(b"HelvB"), helvb_id);
        }
        page.finish();

        pdf.stream(content_ids[i], &page_streams[i]);
    }

    pdf.finish()
}

// -- Font ----------------------------------------------------------------------

/// Built-in Type 1 font selection for this document.
#[derive(Clone, Copy)]
enum PdfFont {
    Regular,
    Bold,
}

impl PdfFont {
    fn name(self) -> Name<'static> {
        match self {
            PdfFont::Regular => Name(b"Helv"),
            PdfFont::Bold    => Name(b"HelvB"),
        }
    }
}

// -- Renderer ------------------------------------------------------------------

/// Builds up a sequence of PDF content streams by tracking drawing operations
/// for the current page plus a running vertical offset (`y`, in millimetres
/// from the top edge). Pages are flushed automatically whenever a row would
/// overflow the bottom margin, repeating the table header on the new page.
struct Renderer<'a> {
    /// Finished content streams, one per completed page.
    pages: Vec<Vec<u8>>,
    /// Content stream being built for the current page.
    content: Content,
    /// Current Y position in mm, measured from the top edge of the page.
    y: f32,
    language: &'a Language,
}

impl<'a> Renderer<'a> {
    fn new(language: &'a Language) -> Self {
        Self {
            pages: Vec::new(),
            content: Content::new(),
            y: MARGIN_TOP_MM,
            language,
        }
    }

    fn finish(mut self) -> Vec<Vec<u8>> {
        self.flush_page();
        self.pages
    }

    fn flush_page(&mut self) {
        let done = std::mem::replace(&mut self.content, Content::new()).finish();
        self.pages.push(done.to_vec());
    }

    /// Finish the current page and start a fresh one at the top margin.
    fn start_new_page(&mut self) {
        self.flush_page();
        self.y = MARGIN_TOP_MM;
    }

    /// Convert a "distance from the top edge in mm" to a Y coordinate in PDF
    /// point space (bottom-left origin, 1 pt = 1/72 in).
    fn y_pt(&self, top_mm: f32) -> f32 {
        mm_to_pt(PAGE_HEIGHT_MM - top_mm)
    }

    /// Draw `text` with its baseline at `(x_mm, baseline_mm)` from the top edge.
    fn draw_text(
        &mut self,
        text: &str,
        x_mm: f32,
        baseline_mm: f32,
        font: PdfFont,
        size_pt: f32,
        color: (u8, u8, u8),
    ) {
        if text.is_empty() {
            return;
        }
        let encoded = encode_winansi(text);
        let (r, g, b) = rgb_f32(color);
        // Nonstroking color must be set outside the text object per PDF spec.
        self.content.set_fill_rgb(r, g, b);
        self.content.begin_text();
        self.content.set_font(font.name(), size_pt);
        self.content.next_line(mm_to_pt(x_mm), self.y_pt(baseline_mm));
        self.content.show(Str(&encoded));
        self.content.end_text();
    }

    /// Filled rectangle whose top-left corner sits at `(x_mm, top_mm)`.
    fn fill_rect(
        &mut self,
        x_mm: f32,
        top_mm: f32,
        width_mm: f32,
        height_mm: f32,
        color: (u8, u8, u8),
    ) {
        let (r, g, b) = rgb_f32(color);
        self.content.set_fill_rgb(r, g, b);
        // PDF `re` origin is the bottom-left corner of the rectangle.
        self.content.rect(
            mm_to_pt(x_mm),
            self.y_pt(top_mm + height_mm),
            mm_to_pt(width_mm),
            mm_to_pt(height_mm),
        );
        self.content.fill_nonzero();
    }

    /// Horizontal divider line at `offset_from_top_mm`, spanning the full
    /// content width starting at the left margin.
    fn content_divider(&mut self, offset_from_top_mm: f32, color: (u8, u8, u8)) {
        let y = self.y_pt(offset_from_top_mm);
        let (r, g, b) = rgb_f32(color);
        self.content.set_stroke_rgb(r, g, b);
        self.content.set_line_width(0.5);
        self.content.move_to(mm_to_pt(MARGIN_LEFT_MM), y);
        self.content.line_to(mm_to_pt(MARGIN_LEFT_MM + CONTENT_WIDTH_MM), y);
        self.content.stroke();
    }

    /// Left edge (in millimetres from the page's left edge) of `column_index`.
    fn column_x(&self, column_index: usize) -> f32 {
        MARGIN_LEFT_MM
            + COLUMNS[..column_index]
                .iter()
                .map(|c| c.width_mm)
                .sum::<f32>()
    }

    /// X position to start drawing `text` in `column_index` so it appears
    /// aligned the way [`Column::align`] specifies, using [`text_width_mm`]
    /// to measure right-/center-aligned text (see the module docs for why
    /// this only works for the limited numeric charset used in those columns).
    fn aligned_x(&self, column_index: usize, text: &str, size_pt: f32) -> f32 {
        let column = &COLUMNS[column_index];
        let left = self.column_x(column_index);
        match column.align {
            Align::Left   => left + 1.0,
            Align::Right  => left + column.width_mm - 1.0 - text_width_mm(text, size_pt),
            Align::Center => left + (column.width_mm - text_width_mm(text, size_pt)) / 2.0,
        }
    }

    /// Draw the shaded column-header row and advance `y` past it.
    fn draw_table_header(&mut self) {
        self.fill_rect(MARGIN_LEFT_MM, self.y, CONTENT_WIDTH_MM, HEADER_HEIGHT_MM, HEADER_FILL);
        let baseline = self.y + 4.8;
        for (index, column) in COLUMNS.iter().enumerate() {
            let label = i18n::translate(self.language, column.header_key, &[]);
            // Headers are always left-aligned regardless of column data alignment.
            let x = self.column_x(index) + 1.0;
            self.draw_text(&label, x, baseline, PdfFont::Bold, 8.0, HEADER_TEXT);
        }
        self.y += HEADER_HEIGHT_MM;
    }

    /// Ensure at least `needed_height_mm` remains before the bottom margin,
    /// starting a new page (and redrawing the table header when requested)
    /// otherwise.
    fn ensure_space(&mut self, needed_height_mm: f32, redraw_header: bool) {
        if self.y + needed_height_mm > PAGE_HEIGHT_MM - PAGE_BOTTOM_MARGIN_MM {
            self.start_new_page();
            if redraw_header {
                self.draw_table_header();
            }
        }
    }

    /// Draw one data row (alternating background shading) and advance `y`.
    fn draw_row(&mut self, cells: &[(usize, String)], shaded: bool) {
        self.ensure_space(ROW_HEIGHT_MM, true);
        if shaded {
            self.fill_rect(
                MARGIN_LEFT_MM,
                self.y,
                CONTENT_WIDTH_MM,
                ROW_HEIGHT_MM,
                ROW_SHADE_FILL,
            );
        }
        let baseline = self.y + 3.8;
        for (column_index, text) in cells {
            let x = self.aligned_x(*column_index, text, 7.5);
            self.draw_text(text, x, baseline, PdfFont::Regular, 7.5, ROW_TEXT);
        }
        self.content_divider(self.y + ROW_HEIGHT_MM, ROW_DIVIDER);
        self.y += ROW_HEIGHT_MM;
    }

    /// Draw a label/value summary line (flextime opening/closing balance).
    /// Unlike data rows this does not redraw the table header on overflow —
    /// it sits below the table, just like in the old browser-side renderer.
    fn draw_summary_row(&mut self, label: &str, value: &str) {
        self.ensure_space(ROW_HEIGHT_MM, false);
        let baseline = self.y + 3.8;
        self.draw_text(
            label,
            MARGIN_LEFT_MM + 1.0,
            baseline,
            PdfFont::Regular,
            7.5,
            SUMMARY_TEXT,
        );
        let value_x = self.aligned_x(DURATION_COLUMN, value, 7.5);
        self.draw_text(value, value_x, baseline, PdfFont::Regular, 7.5, SUMMARY_TEXT);
        self.y += ROW_HEIGHT_MM;
    }

    /// Render one employee's full timesheet section: title block, table
    /// (with multi-page support and repeating header), total row and flextime
    /// balance rows. Always starts at the current page's top margin.
    fn render_section(&mut self, section: &TimesheetSection, from: NaiveDate, to: NaiveDate) {
        // Title block: bold report title, with the employee name and date
        // range as a larger, equally dark second line — so the recipient is
        // immediately visible rather than receding behind the title as a
        // small gray subtitle would.
        let title = i18n::translate(self.language, "pdf_timesheet_title", &[]);
        self.draw_text(
            &title,
            MARGIN_LEFT_MM,
            self.y + 6.0,
            PdfFont::Bold,
            13.0,
            TITLE_COLOR,
        );
        let subtitle = format!("{} - {} - {}", section.user_name, from, to);
        self.draw_text(
            &subtitle,
            MARGIN_LEFT_MM,
            self.y + 13.0,
            PdfFont::Regular,
            11.0,
            TITLE_COLOR,
        );
        self.y += 21.0;
        self.draw_table_header();

        // Alternating shading is keyed to the rendered row count (one increment
        // per drawn row, including each individual entry within a day) — not the
        // day index — so it matches the original browser-side renderer exactly
        // even on days with multiple time entries.
        let mut row_count: usize = 0;
        for day in &section.report.days {
            let weekday = i18n::weekday_label(self.language, &day.weekday);
            // Pass both slug and stored category name so admin-created custom
            // categories (which have no static `absence_kind_<slug>` translation
            // key) print with their real display name instead of the raw slug.
            let absence = match (day.absence.as_deref(), day.absence_name.as_deref()) {
                (Some(slug), Some(name)) => i18n::absence_kind_label(self.language, slug, name),
                _ => String::new(),
            };
            let holiday = day.holiday.clone().unwrap_or_default();
            if day.entries.is_empty() {
                self.draw_row(
                    &[
                        (0, day.date.to_string()),
                        (1, weekday.clone()),
                        (2, String::new()),
                        (3, String::new()),
                        (4, String::new()),
                        (5, format_minutes(0)),
                        (6, absence.clone()),
                        (7, holiday.clone()),
                    ],
                    row_count % 2 == 1,
                );
                row_count += 1;
            } else {
                for entry in &day.entries {
                    self.draw_row(
                        &[
                            (0, day.date.to_string()),
                            (1, weekday.clone()),
                            (2, entry.start_time.get(0..5).unwrap_or("").to_string()),
                            (3, entry.end_time.get(0..5).unwrap_or("").to_string()),
                            (4, i18n::work_category_label(self.language, &entry.category)),
                            (5, format_minutes(entry.minutes)),
                            (6, absence.clone()),
                            (7, holiday.clone()),
                        ],
                        row_count % 2 == 1,
                    );
                    row_count += 1;
                }
            }
        }

        // Total row.
        self.ensure_space(ROW_HEIGHT_MM, true);
        self.fill_rect(
            MARGIN_LEFT_MM,
            self.y,
            CONTENT_WIDTH_MM,
            ROW_HEIGHT_MM,
            TOTAL_FILL,
        );
        let baseline = self.y + 3.8;
        let total_label = i18n::translate(self.language, "pdf_total", &[]);
        self.draw_text(
            &total_label,
            MARGIN_LEFT_MM + 1.0,
            baseline,
            PdfFont::Bold,
            7.5,
            TITLE_COLOR,
        );
        let total_value = format_minutes(range_total_minutes(&section.report));
        let total_x = self.aligned_x(DURATION_COLUMN, &total_value, 7.5);
        self.draw_text(
            &total_value,
            total_x,
            baseline,
            PdfFont::Bold,
            7.5,
            TITLE_COLOR,
        );
        self.y += ROW_HEIGHT_MM;

        let (opening, closing) = flextime_bounds(&section.flextime_data);
        if let Some(opening_balance) = opening {
            let label = i18n::translate(self.language, "pdf_flextime_opening_balance", &[]);
            self.draw_summary_row(&label, &format_signed_minutes(opening_balance));
        }
        if let Some(closing_balance) = closing {
            let label = i18n::translate(self.language, "pdf_flextime_closing_balance", &[]);
            self.draw_summary_row(&label, &format_signed_minutes(closing_balance));
        }
    }
}

// -- Helper functions ----------------------------------------------------------

/// Convert millimetres to PDF user-space points (1 pt = 1/72 in).
fn mm_to_pt(mm: f32) -> f32 {
    mm * 72.0 / 25.4
}

/// Encode a Rust string into WinAnsiEncoding bytes for PDF Type 1 built-in
/// fonts. ASCII (0x00–0x7F) passes through unchanged. The Latin-1 supplement
/// (U+00A0–U+00FF) — which covers all German umlauts and most Western-European
/// accented letters — maps to bytes 0xA0–0xFF directly. Characters outside
/// those ranges are replaced with `?`.
fn encode_winansi(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| {
            let cp = c as u32;
            if cp <= 0x7F {
                cp as u8
            } else if (0xA0..=0xFF).contains(&cp) {
                cp as u8
            } else {
                b'?'
            }
        })
        .collect()
}

/// Decompose an sRGB byte triplet into floating-point components in [0.0, 1.0].
fn rgb_f32(color: (u8, u8, u8)) -> (f32, f32, f32) {
    (
        f32::from(color.0) / 255.0,
        f32::from(color.1) / 255.0,
        f32::from(color.2) / 255.0,
    )
}

/// Sum of approved, crediting entry minutes across the whole report range —
/// the same definition the CSV/UI "Total" row uses.
fn range_total_minutes(report: &MonthReport) -> i64 {
    report
        .days
        .iter()
        .flat_map(|day| &day.entries)
        .filter(|entry| entry.status == "approved" && entry.counts_as_work)
        .map(|entry| entry.minutes)
        .sum()
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

/// Format minutes as `H:MM`, e.g. `8:00`, `0:00` — mirrors the frontend's
/// `minToHM` (used for durations, which are never negative).
fn format_minutes(total_minutes: i64) -> String {
    let sign = if total_minutes < 0 { "-" } else { "" };
    let absolute_minutes = total_minutes.abs();
    format!(
        "{sign}{}:{:02}",
        absolute_minutes / 60,
        absolute_minutes % 60
    )
}

/// Format minutes as a signed `H:MM` balance, e.g. `+8:00` / `-0:15` — mirrors
/// how the frontend renders flextime balances (`(value >= 0 ? "+" : "") + minToHM(value)`).
fn format_signed_minutes(total_minutes: i64) -> String {
    if total_minutes >= 0 {
        format!("+{}", format_minutes(total_minutes))
    } else {
        format_minutes(total_minutes)
    }
}

/// Width of a glyph in thousandths of an em, taken from the public Adobe AFM
/// core-14 metrics for Helvetica. Only covers the characters that
/// [`format_minutes`] / [`format_signed_minutes`] and `HH:MM` time strings can
/// ever produce — see the module docs for why that is sufficient here.
fn glyph_width_1000(glyph: char) -> u32 {
    match glyph {
        '0'..='9' => 556,
        ':' => 278,
        '+' => 584,
        '-' => 333,
        '.' => 278,
        _ => 278, // space and any other separator that might appear
    }
}

/// Rendered width of `text` set in `size_pt`, in millimetres. Glyph widths are
/// in thousandths of an em (where "em" == font size); converts pt → mm via
/// the standard 72 pt/inch, 25.4 mm/inch relationship.
fn text_width_mm(text: &str, size_pt: f32) -> f32 {
    let width_em_thousandths: u32 = text.chars().map(glyph_width_1000).sum();
    (width_em_thousandths as f32 / 1000.0) * size_pt * (25.4 / 72.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_minutes_matches_frontend_min_to_hm() {
        assert_eq!(format_minutes(0), "0:00");
        assert_eq!(format_minutes(480), "8:00");
        assert_eq!(format_minutes(75), "1:15");
        assert_eq!(format_minutes(-15), "-0:15");
    }

    #[test]
    fn format_signed_minutes_adds_a_leading_plus_for_non_negative_values() {
        assert_eq!(format_signed_minutes(0), "+0:00");
        assert_eq!(format_signed_minutes(754), "+12:34");
        assert_eq!(format_signed_minutes(-15), "-0:15");
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
    fn text_width_grows_with_each_digit() {
        let one_digit = text_width_mm("8", 7.5);
        let four_digits = text_width_mm("12:34", 7.5);
        assert!(four_digits > one_digit * 2.0);
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
