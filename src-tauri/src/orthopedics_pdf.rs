//! Native, versioned orthopedic PDF renderer (A4).
//! This document is generated from an immutable saved clinical-report version,
//! never from a screenshot of the React WebView.
use std::io::{BufWriter, Cursor};
use anyhow::{Context, Result};
use printpdf::{
    BuiltinFont, Color, Image, ImageTransform, IndirectFontRef, Mm, PdfDocument,
    PdfDocumentReference, PdfLayerReference, Rgb,
};
use serde_json::Value;

const PAGE_WIDTH: f32 = 210.0;
const PAGE_HEIGHT: f32 = 297.0;
const LEFT: f32 = 17.0;
const POS_LOGO: &[u8] = include_bytes!("../resources/programeri-pos-logo.png");

fn value<'a>(obj: &'a Value, key: &str) -> &'a str {
    obj.get(key).and_then(Value::as_str).unwrap_or("")
}
fn field(obj: &Value, key: &str, display: &str) -> Option<String> {
    let s = value(obj, key).trim();
    if !s.is_empty() { Some(format!("{display}: {s}")) } else { None }
}
fn report_type(kind: &str) -> &str {
    match kind {
        "initial_assessment" => "Vleresimi fillestar",
        "fracture_followup" => "Kontrolli i fraktures",
        "surgical_plan" => "Plani kirurgjik",
        "operative_report" => "Raporti operativ",
        "implant_record" => "Regjistri i implanteve",
        "postoperative_followup" => "Kontrolli postoperator",
        "rehabilitation_progress" => "Progresi i rehabilitimit",
        "final_summary" => "Permbledhja finale",
        _ => "Raport ortopedik",
    }
}
fn side_name(s: &str) -> &str {
    match s { "right"=>"Djathtas", "left"=>"Majtas", "bilateral"=>"Dyaneshem",
        "midline"=>"Ne vije te mesme", _=>s }
}

// Explicit line breaking and pagination prevent the report from being cropped
// by an arbitrary browser viewport / CSS. PDF page size is always A4.
struct ReportPage {
    doc: PdfDocumentReference,
    layer: PdfLayerReference,
    regular: IndirectFontRef,
    bold: IndirectFontRef,
    y: f32,
    page_no: usize,
}

impl ReportPage {
    fn line(&mut self, text: &str, font_size: f32, is_bold: bool) {
        if self.y < 23.0 {
            let (page, layer) = self.doc.add_page(Mm(PAGE_WIDTH), Mm(PAGE_HEIGHT), "Report");
            self.layer = self.doc.get_page(page).get_layer(layer);
            self.page_no += 1;
            self.y = 277.0;
            self.layer.set_fill_color(Color::Rgb(Rgb::new(0.10, 0.48, 0.53, None)));
            self.layer.use_text(
                format!("PROGRAMERI POS   |   Ortopedi   |   Faqe {}", self.page_no),
                8.0, Mm(LEFT), Mm(284.0), &self.bold
            );
        }
        let text = text.trim_end_matches(['\r', '\n']);
        let font = if is_bold { &self.bold } else { &self.regular };
        self.layer.set_fill_color(Color::Rgb(Rgb::new(0.12, 0.19, 0.24, None)));
        self.layer.use_text(text, font_size, Mm(LEFT), Mm(self.y), font);
        self.y -= if font_size >= 12.0 { 7.4 } else { 5.5 };
    }

    fn wrapped(&mut self, input: &str, font_size: f32) {
        // Approx. 92 glyphs at 9.5 pt within 176 mm; split on UTF-8 chars.
        // Preserve paragraph breaks and long tokens, never silently truncate.
        let max_chars: usize = if font_size >= 10.0 { 82 } else { 94 };
        for paragraph in input.replace('\r', "").split('\n') {
            if paragraph.trim().is_empty() {
                self.y -= 3.5;
                continue;
            }
            let mut row = String::new();
            for token in paragraph.split_whitespace() {
                for segment in chunks(token, max_chars) {
                    let len = row.chars().count();
                    let extra = if len > 0 { 1 } else { 0 };
                    if len + extra + segment.chars().count() > max_chars && !row.is_empty() {
                        self.line(&row, font_size, false);
                        row.clear();
                    }
                    if !row.is_empty() { row.push(' '); }
                    row.push_str(&segment);
                }
            }
            if !row.is_empty() { self.line(&row, font_size, false); }
        }
    }

    fn section(&mut self, title: &str) {
        self.y -= 3.0;
        self.line(title, 11.5, true);
        self.y -= 1.0;
    }
}

fn chunks(text: &str, max: usize) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut tmp = String::new();
    for ch in text.chars() {
        tmp.push(ch);
        if tmp.chars().count() == max {
            pieces.push(std::mem::take(&mut tmp));
        }
    }
    if !tmp.is_empty() { pieces.push(tmp); }
    pieces
}

fn render_entries(page: &mut ReportPage, body: &Value, name: &str, heading: &str, fields: &[(&str, &str)]) {
    let Some(rows) = body.get(name).and_then(Value::as_array) else { return; };
    if rows.is_empty() { return; }
    page.section(heading);
    for (i, row) in rows.iter().enumerate() {
        let parts: Vec<String> = fields.iter()
            .filter_map(|(field_name, label)| field(row, field_name, label))
            .collect();
        page.wrapped(&format!("{}. {}", i + 1, parts.join("  |  ")), 9.0);
        page.y -= 1.8;
    }
}

/// Renders an independently styled A4 clinical report from a stored report and
/// the SQL-linked patient/episode metadata. Draft status remains visibly draft.
pub fn render_orthopedic_pdf(report: &Value, clinic_name: &str) -> Result<Vec<u8>> {
    let id = value(report, "id");
    let body_json = value(report, "body_json");
    let body: Value = serde_json::from_str(body_json).context("parse saved clinical report")?;
    let (doc, p, l) = PdfDocument::new(
        "Raport ortopedik - PROGRAMERI POS", Mm(PAGE_WIDTH), Mm(PAGE_HEIGHT), "Raport"
    );
    let regular = doc.add_builtin_font(BuiltinFont::Helvetica)?;
    let bold = doc.add_builtin_font(BuiltinFont::HelveticaBold)?;
    let mut page = ReportPage {
        layer: doc.get_page(p).get_layer(l),
        doc, regular, bold, y: 274.0, page_no: 1,
    };

    // Authentic POS identity from the application's existing source repository.
    let mut logo_cursor = Cursor::new(POS_LOGO);
    if let Ok(decoder) = printpdf::image_crate::codecs::png::PngDecoder::new(&mut logo_cursor) {
        if let Ok(img) = Image::try_from(decoder) {
            let dpi = 300.0_f32;
            let w_px = img.image.width.0 as f32;
            let h_px = img.image.height.0 as f32;
            if w_px > 0.0 && h_px > 0.0 {
                let side_mm = 20.0;
                img.add_to_layer(page.layer.clone(), ImageTransform {
                    translate_x: Some(Mm(LEFT)),
                    translate_y: Some(Mm(266.0)),
                    rotate: None,
                    scale_x: Some(side_mm * dpi / (w_px * 25.4)),
                    scale_y: Some(side_mm * dpi / (h_px * 25.4)),
                    dpi: Some(dpi),
                });
            }
        }
    }
    page.layer.set_fill_color(Color::Rgb(Rgb::new(0.03, 0.55, 0.60, None)));
    page.layer.use_text("PROGRAMERI POS", 14.0, Mm(44.0), Mm(279.0), &page.bold);
    page.layer.use_text("ORTOPEDI & TRAUMATOLOGJI", 8.5, Mm(44.0), Mm(271.0), &page.regular);
    page.y = 254.0;
    let title = report_type(value(report, "type"));
    page.line(&format!("RAPORT KLINIK: {}", title), 13.0, true);
    page.y -= 1.0;
    if !clinic_name.trim().is_empty() {
        page.wrapped(&format!("Institucioni: {}", clinic_name.trim()), 9.5);
    }
    page.wrapped(&format!("Pacienti: {}", value(report, "patient_name")), 10.0);
    let code = value(report, "patient_code");
    if !code.is_empty() { page.wrapped(&format!("Kodi i pacientit: {}", code), 9.5); }
    page.wrapped(&format!("Diagnoza: {}", value(report, "diagnosis")), 10.0);
    page.wrapped(&format!(
        "Regjioni: {}  |  Ana: {}  |  Episodi: {}",
        value(report, "region"), side_name(value(report, "laterality")),
        value(report, "episode_id")
    ), 9.0);
    page.wrapped(&format!(
        "Versioni: {}  |  Statusi: {}  |  Data: {}",
        report["version"].as_i64().unwrap_or(0),
        value(report, "status").to_uppercase(), value(report, "created_at")
    ), 9.0);
    if value(report, "status") == "draft" {
        page.line("DRAFT - DOKUMENT I PAFINALIZUAR", 10.0, true);
    }
    if value(report, "status") == "amendment" {
        page.wrapped(&format!("ARSYEJA E AMENDAMENTIT: {}", value(report, "reason")), 9.5);
    }

    page.section("GJETJET KLINIKE / PERFUNDIMI");
    let narrative = value(&body, "narrative");
    page.wrapped(if narrative.trim().is_empty() { "Nuk ka pershkrim klinik ne versionin e ruajtur." } else { narrative }, 9.5);

    render_entries(&mut page, &body, "fractures", "FRAKTURAT", &[
        ("boneRegion","Kocka"), ("laterality","Ana"), ("segment","Segmenti"),
        ("classification","Klasifikimi"), ("currentStatus","Statusi"),
        ("treatmentStrategy","Trajtimi")
    ]);
    render_entries(&mut page, &body, "assessments", "VLERESIMET", &[
        ("category","Tipi"), ("joint","Nyja"), ("movement","Levizja"),
        ("degrees","Grade"), ("pain","Dhimbja"), ("testName","Testi"),
        ("testResult","Rezultati"), ("note","Shenimi")
    ]);
    render_entries(&mut page, &body, "operations", "NDERHYRJET OPERATIVE", &[
        ("actual_procedure","Procedura"), ("operative_site","Regjioni"),
        ("laterality","Ana"), ("operative_note","Raporti"),
        ("surgeon_id","Kirurgu"), ("finalized_at","Data")
    ]);
    render_entries(&mut page, &body, "implants", "IMPLANTET", &[
        ("manufacturer","Prodhuesi"), ("model","Modeli"), ("lot","Lot"),
        ("serial","Serial"), ("status","Statusi"), ("removalStatus","Gjendja")
    ]);
    render_entries(&mut page, &body, "rehab", "REHABILITIMI", &[
        ("protocol","Protokolli"), ("phase","Faza"),
        ("loadRestrictions","Ngarkesa"), ("romRestrictions","ROM")
    ]);

    page.y -= 5.0;
    page.section("AUTORIZIMI DHE GJURMUESHMERIA");
    page.wrapped(&format!("Autori (ID): {}", value(report, "author_id")), 9.5);
    page.wrapped(&format!("ID raporti: {}  |  Versioni i ruajtur: {}", id,
        report["version"].as_i64().unwrap_or(0)), 8.0);
    page.wrapped("Dokumenti eshte gjeneruar nga versioni i ruajtur ne databazen klinike. Nuk eshte kopje e ekranit.", 8.0);

    let mut buf = BufWriter::new(Cursor::new(Vec::<u8>::new()));
    page.doc.save(&mut buf).context("encode orthopedic PDF")?;
    let cursor = buf.into_inner().map_err(|e| anyhow::anyhow!("flush orthopedic PDF: {e}"))?;
    Ok(cursor.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn renders_independent_a4_pdf_with_saved_version_and_long_narrative() {
        let text = "Dokumentacion ortopedik per nje pacient testues. ".repeat(230);
        let report = serde_json::json!({
            "id":"ortho-report-1","episode_id":"ortho-episode-1",
            "patient_name":"Test Patient","diagnosis":"Fracture",
            "region":"Knee","laterality":"left","version":2,
            "type":"final_summary","status":"final","author_id":"doctor-test",
            "created_at":"2026-10-25T17:00:00Z",
            "body_json":serde_json::json!({"narrative":text,
                "fractures":[{"boneRegion":"Femur","currentStatus":"healing"}]}).to_string()
        });
        let bytes = render_orthopedic_pdf(&report, "Test Clinic").unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
        assert!(bytes.windows(5).any(|w| w==b"%%EOF"));
        assert!(bytes.len()>1500);
    }
}
