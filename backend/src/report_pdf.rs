//! Server-side timesheet PDF rendering.
//!
//! Pure rendering logic with no database access (mirrors `email.rs` in that
//! respect). Lays out one printable timesheet per [`TimesheetSection`] — title,
//! a table of days/entries, a total row, and flextime balance rows — using
//! `pdf-writer`'s low-level PDF primitives with the PDF standard 14 fonts.
//! Standard 14 fonts (Helvetica and Helvetica-Bold) are referenced by name only;
//! PDF viewers are required to have them available, so no font data is embedded
//! in the output, keeping the binary small and avoiding font-licensing questions.
//!
//! Coordinates are tracked in millimetres measured from the top-left corner
//! (matching how the previous browser-side renderer worked, and how the layout
//! below was designed) and converted to PDF's bottom-left-origin point space
//! only at the moment an operation is emitted, via [`Renderer::baseline_pt`] /
//! [`Renderer::flip_y_pt`].
//!
//! PDF standard 14 fonts do not expose glyph-width metrics through `pdf-writer`,
//! so right-/center-aligned text cannot be positioned the way `jsPDF.text(...,
//! { align })` did in the browser. The only cell values that need such alignment
//! — `Start`, `End` (`HH:MM`) and `Duration` (`±H:MM`, see [`format_minutes`])
//! — are composed solely of digits, `:`, `+`, `-`, `.` and spaces, so
//! [`glyph_width_1000`] hardcodes the public-domain Adobe AFM Helvetica metrics
//! for just that subset, which is enough to compute alignment offsets ourselves.
//! Every other string (including all column headers) is left-aligned,
//! sidestepping the missing-metrics problem for translated, variable-width text.

use crate::i18n::{self, Language};
use crate::services::reports::{FlextimeDay, MonthReport};
use chrono::NaiveDate;
use pdf_writer::types::LineCapStyle;
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref, Str};

// -- Page geometry (millimetres) ----------------------------------------------

const PAGE_WIDTH_MM: f32 = 210.0;
const PAGE_HEIGHT_MM: f32 = 297.0;
const MARGIN_LEFT_MM: f32 = 15.0;
const MARGIN_TOP_MM: f32 = 15.0;
const CONTENT_WIDTH_MM: f32 = 180.0;
const ROW_HEIGHT_MM: f32 = 5.5;
const HEADER_HEIGHT_MM: f32 = 7.0;
const PAGE_BOTTOM_MARGIN_MM: f32 = 15.0;

// -- Palette (matches the previous browser-rendered timesheet PDF) ------------

const TITLE_COLOR: (u8, u8, u8) = (20, 20, 20);
const HEADER_FILL: (u8, u8, u8) = (235, 235, 235);
const HEADER_TEXT: (u8, u8, u8) = (50, 50, 50);
const ROW_TEXT: (u8, u8, u8) = (30, 30, 30);
const ROW_SHADE_FILL: (u8, u8, u8) = (248, 248, 248);
const ROW_DIVIDER: (u8, u8, u8) = (220, 220, 220);
const SUMMARY_TEXT: (u8, u8, u8) = (90, 90, 90);
const TOTAL_FILL: (u8, u8, u8) = (235, 235, 235);

// -- PDF unit conversion ------------------------------------------------------

// Points per millimetre (72 pt/inch ÷ 25.4 mm/inch).
const PT_PER_MM: f32 = 72.0 / 25.4;

fn mm_to_pt(mm: f32) -> f32 {
    mm * PT_PER_MM
}

// -- Font identifiers ---------------------------------------------------------

// PDF resource names for the two standard 14 fonts used below.
const FONT_REGULAR: Name = Name(b"F1");
const FONT_BOLD: Name = Name(b"F2");

#[derive(Clone, Copy, PartialEq, Eq)]
enum Font {
    Regular,
    Bold,
}

// -- Column layout ------------------------------------------------------------

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
    Column {
        header_key: "pdf_column_date",
        width_mm: 22.0,
        align: Align::Left,
    },
    Column {
        header_key: "pdf_column_weekday",
        width_mm: 20.0,
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
        width_mm: 40.0,
        align: Align::Left,
    },
    Column {
        header_key: "pdf_column_duration",
        width_mm: 16.0,
        align: Align::Right,
    },
    Column {
        header_key: "pdf_column_absence",
        width_mm: 25.0,
        align: Align::Left,
    },
    Column {
        header_key: "pdf_column_holiday",
        width_mm: 33.0,
        align: Align::Left,
    },
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
    renderer.finish()
}

// -- PDF object reference allocation ------------------------------------------

struct Refs {
    catalog: Ref,
    page_tree: Ref,
    // Per-page content stream and page object refs, in order.
    pages: Vec<(Ref, Ref)>,
    font_regular: Ref,
    font_bold: Ref,
}

impl Refs {
    fn new(page_count: usize) -> Self {
        // Allocate refs sequentially starting at 1.
        let catalog = Ref::new(1);
        let page_tree = Ref::new(2);
        let font_regular = Ref::new(3);
        let font_bold = Ref::new(4);
        // Each page needs a content-stream ref and a page-object ref.
        let pages = (0..page_count)
            .map(|i| {
                let base = 5 + (i as i32) * 2;
                (Ref::new(base), Ref::new(base + 1))
            })
            .collect();
        Refs {
            catalog,
            page_tree,
            pages,
            font_regular,
            font_bold,
        }
    }
}

// -- Renderer -----------------------------------------------------------------

/// Accumulates per-page [`Content`] streams and then assembles the final PDF
/// document in [`Renderer::finish`].
struct Renderer<'a> {
    /// Finished content bytes for each completed page.
    page_contents: Vec<Vec<u8>>,
    /// Content operations for the page currently being built.
    current: Content,
    /// Vertical offset from the top edge of the page, in millimetres.
    y: f32,
    language: &'a Language,
    /// Tracks the font and size currently set in `current`, to avoid redundant
    /// `Tf` operators which inflate the stream size.
    current_font: Option<(Font, f32)>,
}

impl<'a> Renderer<'a> {
    fn new(language: &'a Language) -> Self {
        Self {
            page_contents: Vec::new(),
            current: Content::new(),
            y: MARGIN_TOP_MM,
            language,
            current_font: None,
        }
    }

    /// Finalise all pages and assemble the complete PDF document bytes.
    fn finish(mut self) -> Vec<u8> {
        // Flush the last open page.
        self.flush_page();

        let page_count = self.page_contents.len();
        let refs = Refs::new(page_count);

        let mut pdf = Pdf::new();

        // Catalog and page tree.
        pdf.catalog(refs.catalog).pages(refs.page_tree);
        let page_width_pt = mm_to_pt(PAGE_WIDTH_MM);
        let page_height_pt = mm_to_pt(PAGE_HEIGHT_MM);
        let media_box = Rect::new(0.0, 0.0, page_width_pt, page_height_pt);

        let mut pages = pdf.pages(refs.page_tree);
        pages.media_box(media_box);
        pages.kids(refs.pages.iter().map(|(_, page_ref)| *page_ref));
        pages.count(page_count as i32);
        pages.finish();

        // Font resources: Helvetica (regular) and Helvetica-Bold.
        // PDF standard 14 fonts are referenced by their PostScript name and must
        // not be embedded — all conforming viewers supply them.
        // WinAnsiEncoding ensures bytes 0x20-0xFF map to the expected Latin-1/Windows-1252
        // characters, so accented letters (umlauts, French accents, etc.) in user
        // names and category names are rendered correctly.
        let mut font = pdf.type1_font(refs.font_regular);
        font.base_font(Name(b"Helvetica"));
        font.encoding_predefined(Name(b"WinAnsiEncoding"));
        font.finish();

        let mut font = pdf.type1_font(refs.font_bold);
        font.base_font(Name(b"Helvetica-Bold"));
        font.encoding_predefined(Name(b"WinAnsiEncoding"));
        font.finish();

        // Write each page and its content stream.
        for (i, (content_ref, page_ref)) in refs.pages.iter().enumerate() {
            // Compress the content stream with Deflate.
            let compressed = miniz_compress(&self.page_contents[i]);

            let mut stream = pdf.stream(*content_ref, &compressed);
            stream.filter(Filter::FlateDecode);
            stream.finish();

            let mut page = pdf.page(*page_ref);
            page.media_box(media_box);
            page.parent(refs.page_tree);
            page.contents(*content_ref);

            // Every page uses the same two font resources.
            let mut resources = page.resources();
            let mut fonts = resources.fonts();
            fonts.pair(FONT_REGULAR, refs.font_regular);
            fonts.pair(FONT_BOLD, refs.font_bold);
            fonts.finish();
            resources.finish();
            page.finish();
        }

        pdf.finish()
    }

    fn flush_page(&mut self) {
        let finished = std::mem::replace(&mut self.current, Content::new())
            .finish()
            .into_vec();
        self.page_contents.push(finished);
        self.current_font = None;
    }

    /// Finish the current page and start a fresh one at the top margin.
    fn start_new_page(&mut self) {
        self.flush_page();
        self.y = MARGIN_TOP_MM;
    }

    /// Convert a distance from the top edge (mm) to PDF's bottom-left origin (pt).
    fn flip_y_pt(&self, offset_from_top_mm: f32) -> f32 {
        mm_to_pt(PAGE_HEIGHT_MM - offset_from_top_mm)
    }

    /// Y coordinate for a text baseline at `offset_from_top_mm`.
    fn baseline_pt(&self, offset_from_top_mm: f32) -> f32 {
        self.flip_y_pt(offset_from_top_mm)
    }

    /// Set fill colour (RGB, each component 0.0–1.0) only when it changes.
    fn set_fill_color(&mut self, color: (u8, u8, u8)) {
        let r = f32::from(color.0) / 255.0;
        let g = f32::from(color.1) / 255.0;
        let b = f32::from(color.2) / 255.0;
        self.current.set_fill_rgb(r, g, b);
    }

    /// Set stroke colour (RGB, each component 0.0–1.0).
    fn set_stroke_color(&mut self, color: (u8, u8, u8)) {
        let r = f32::from(color.0) / 255.0;
        let g = f32::from(color.1) / 255.0;
        let b = f32::from(color.2) / 255.0;
        self.current.set_stroke_rgb(r, g, b);
    }

    /// Draw `text` at `(x_mm, baseline_offset_from_top_mm)` in the given font.
    fn draw_text(
        &mut self,
        text: &str,
        x_mm: f32,
        baseline_offset_from_top_mm: f32,
        font: Font,
        size_pt: f32,
        color: (u8, u8, u8),
    ) {
        if text.is_empty() {
            return;
        }
        self.set_fill_color(color);

        let x_pt = mm_to_pt(x_mm);
        let y_pt = self.baseline_pt(baseline_offset_from_top_mm);

        // PDF standard 14 fonts use WinAnsiEncoding (Latin-1 supplement matches
        // Windows-1252 for 0xA0-0xFF). Encode the string lossily: replace any
        // codepoint above U+00FF with '?' — Latin-1 printable characters pass
        // through unchanged as their byte value equals the glyph code point.
        let encoded: Vec<u8> = text
            .chars()
            .map(|c| if c as u32 <= 0xFF { c as u8 } else { b'?' })
            .collect();

        self.current.begin_text();
        // Emit Tf inside the text block for maximum viewer compatibility.
        // Only change font/size when it differs from the previous call.
        if self.current_font != Some((font, size_pt)) {
            let font_name = match font {
                Font::Regular => FONT_REGULAR,
                Font::Bold => FONT_BOLD,
            };
            self.current.set_font(font_name, size_pt);
            self.current_font = Some((font, size_pt));
        }
        self.current.set_text_matrix([1.0, 0.0, 0.0, 1.0, x_pt, y_pt]);
        self.current.show(Str(&encoded));
        self.current.end_text();
    }

    /// Draw a filled rectangle whose top-left corner is at
    /// `(x_mm, top_offset_from_top_mm)`.
    fn fill_rect(
        &mut self,
        x_mm: f32,
        top_offset_from_top_mm: f32,
        width_mm: f32,
        height_mm: f32,
        color: (u8, u8, u8),
    ) {
        self.set_fill_color(color);
        let x_pt = mm_to_pt(x_mm);
        // PDF rect origin is bottom-left; convert the top edge and subtract height.
        let y_pt = self.flip_y_pt(top_offset_from_top_mm + height_mm);
        let w_pt = mm_to_pt(width_mm);
        let h_pt = mm_to_pt(height_mm);
        self.current.rect(x_pt, y_pt, w_pt, h_pt);
        self.current.fill_nonzero();
    }

    /// Draw a horizontal divider line at `offset_from_top_mm`, spanning the
    /// full content width starting at the left margin.
    fn content_divider(&mut self, offset_from_top_mm: f32, color: (u8, u8, u8)) {
        self.set_stroke_color(color);
        self.current.set_line_width(0.5);
        self.current.set_line_cap(LineCapStyle::ButtCap);
        let y_pt = self.flip_y_pt(offset_from_top_mm);
        let x0 = mm_to_pt(MARGIN_LEFT_MM);
        let x1 = mm_to_pt(MARGIN_LEFT_MM + CONTENT_WIDTH_MM);
        self.current.move_to(x0, y_pt);
        self.current.line_to(x1, y_pt);
        self.current.stroke();
    }

    /// Left edge (in millimetres from the page's left edge) of `column_index`.
    fn column_x(&self, column_index: usize) -> f32 {
        MARGIN_LEFT_MM
            + COLUMNS[..column_index]
                .iter()
                .map(|column| column.width_mm)
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
            Align::Left => left + 1.0,
            Align::Right => left + column.width_mm - 1.0 - text_width_mm(text, size_pt),
            Align::Center => left + (column.width_mm - text_width_mm(text, size_pt)) / 2.0,
        }
    }

    /// Draw the shaded column-header row and advance `y` past it.
    fn draw_table_header(&mut self) {
        self.fill_rect(
            MARGIN_LEFT_MM,
            self.y,
            CONTENT_WIDTH_MM,
            HEADER_HEIGHT_MM,
            HEADER_FILL,
        );
        let baseline = self.y + 4.8;
        for (index, column) in COLUMNS.iter().enumerate() {
            let label = i18n::translate(self.language, column.header_key, &[]);
            // Headers are always left-aligned (see module docs) regardless of
            // the column's data alignment.
            let x = self.column_x(index) + 1.0;
            self.draw_text(&label, x, baseline, Font::Bold, 8.0, HEADER_TEXT);
        }
        self.y += HEADER_HEIGHT_MM;
    }

    /// Ensure at least `needed_height_mm` remains before the bottom margin,
    /// starting a new page (and redrawing the table header when requested)
    /// otherwise. Mirrors the overflow checks the old browser-side renderer
    /// performed before every row.
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
            self.draw_text(text, x, baseline, Font::Regular, 7.5, ROW_TEXT);
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
            Font::Regular,
            7.5,
            SUMMARY_TEXT,
        );
        let value_x = self.aligned_x(DURATION_COLUMN, value, 7.5);
        self.draw_text(value, value_x, baseline, Font::Regular, 7.5, SUMMARY_TEXT);
        self.y += ROW_HEIGHT_MM;
    }

    /// Render one employee's full timesheet section: title block, table
    /// (with multi-page support and repeating header), total row and
    /// flextime balance rows. Always starts at the current page's top margin.
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
            Font::Bold,
            13.0,
            TITLE_COLOR,
        );
        let subtitle = format!("{} - {} - {}", section.user_name, from, to);
        self.draw_text(
            &subtitle,
            MARGIN_LEFT_MM,
            self.y + 13.0,
            Font::Regular,
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
            Font::Bold,
            7.5,
            TITLE_COLOR,
        );
        let total_value = format_minutes(range_total_minutes(&section.report));
        let total_x = self.aligned_x(DURATION_COLUMN, &total_value, 7.5);
        self.draw_text(
            &total_value,
            total_x,
            baseline,
            Font::Bold,
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

// -- Deflate compression for content streams ----------------------------------

/// Compress `data` with Deflate (zlib wrapper) using the `flate2` crate, which
/// is already an indirect transitive dependency. Falls back to uncompressed if
/// compression somehow fails.
fn miniz_compress(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut encoder =
        flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    if encoder.write_all(data).is_err() {
        return data.to_vec();
    }
    encoder.finish().unwrap_or_else(|_| data.to_vec())
}

// -- Domain helpers -----------------------------------------------------------

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
