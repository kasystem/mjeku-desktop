use std::io::{BufWriter, Cursor};

use anyhow::{anyhow, Context};
use printpdf::path::{PaintMode, WindingOrder};
use printpdf::{
    BuiltinFont, Color, Image, ImageTransform, IndirectFontRef, Line, Mm, PdfDocument,
    PdfDocumentReference, PdfLayerReference, PdfPageIndex, Point, Polygon, Rgb,
};

// ─── Data structures ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct InvoiceLine {
    pub tooth: Option<String>,
    pub title: String,
    pub qty: f64,
    pub unit_price: f64,
    pub fiscal: bool,
    pub vat_code: String, // A | C | D | E
}

#[derive(Debug, Clone)]
pub struct InvoicePdfData {
    pub clinic_name: String,
    pub header_png: Option<Vec<u8>>,
    pub logo_png: Option<Vec<u8>>,
    pub invoice_id: String,
    pub date: Option<String>,
    pub client_name: String,
    pub client_code: Option<String>,
    pub client_dob: Option<String>,
    pub client_address: Option<String>,
    pub client_city: Option<String>,
    pub client_phone: Option<String>,
    pub client_email: Option<String>,
    pub notes: Option<String>,
    pub bank_account: Option<String>,
    pub lines: Vec<InvoiceLine>,
    pub total: f64,
    pub fiscal_total: f64,
    pub non_fiscal_total: f64,
}

#[derive(Debug, Clone)]
pub struct VisitPdfData {
    pub clinic_name: String,
    pub header_png: Option<Vec<u8>>,
    pub logo_png: Option<Vec<u8>>,
    pub visit_id: String,
    pub date: Option<String>,
    pub visit_time: Option<String>,
    pub status: String,
    pub doctor_name: Option<String>,
    pub client_name: String,
    pub client_code: Option<String>,
    pub client_dob: Option<String>,
    pub client_address: Option<String>,
    pub client_city: Option<String>,
    pub client_phone: Option<String>,
    pub client_email: Option<String>,
    pub notes: Option<String>,
    pub body_weight: Option<String>,
    pub body_weight_unit: Option<String>,
    pub body_height: Option<String>,
    pub body_height_unit: Option<String>,
    pub head_circumference: Option<String>,
    pub head_circumference_unit: Option<String>,
    pub body_temperature: Option<String>,
    pub body_temperature_unit: Option<String>,
    pub blood_oxygen: Option<String>,
    pub blood_oxygen_unit: Option<String>,
    pub glycemia: Option<String>,
    pub glycemia_unit: Option<String>,
    pub pulse: Option<String>,
    pub pulse_unit: Option<String>,
    pub bmi: Option<String>,
    pub blood_pressure_systolic: Option<String>,
    pub blood_pressure_diastolic: Option<String>,
    pub blood_pressure_unit: Option<String>,
    pub complaints: Option<String>,
    pub additional_notes: Option<String>,
    pub controls: Option<String>,
    pub remarks: Option<String>,
    pub analyses: Option<String>,
    pub advice: Option<String>,
    pub therapies: Option<String>,
    pub diagnosis: Option<String>,
    pub examinations: Option<String>,
    pub lines: Vec<InvoiceLine>,
    pub total: f64,
}

// ─── Layout constants (f32 — printpdf Mm(pub f32)) ───────────────────────────

const PAGE_W: f32 = 210.0;
const PAGE_H: f32 = 297.0;
const ML: f32 = 14.0;        // left margin mm
const MR: f32 = 14.0;        // right margin mm
const CR: f32 = PAGE_W - MR; // content right edge = 196 mm
const CW: f32 = CR - ML;     // content width = 182 mm
const LH: f32 = 6.0;         // standard line height mm

// Invoice table column edges (mm from page left)
const T_NR_R: f32    = 23.0;
const T_DESC_L: f32  = 25.0;
const T_QTY_R: f32   = 130.0;
const T_PRICE_R: f32 = 152.0;
const T_VAT_L: f32   = 154.0;
const T_VAT_R: f32   = 167.0;
const T_TOT_R: f32   = 196.0;

// ─── Colors — warm forest-green palette ──────────────────────────────────────

fn c_navy()       -> Color { Color::Rgb(Rgb::new(0.12, 0.32, 0.22, None)) } // deep forest green
fn c_navy_mid()   -> Color { Color::Rgb(Rgb::new(0.20, 0.45, 0.32, None)) } // medium green
fn c_navy_pale()  -> Color { Color::Rgb(Rgb::new(0.75, 0.88, 0.80, None)) } // pale sage
fn c_white()      -> Color { Color::Rgb(Rgb::new(1.00, 1.00, 1.00, None)) }
fn c_row_alt()    -> Color { Color::Rgb(Rgb::new(0.98, 0.97, 0.94, None)) } // warm ivory
fn c_hdr_row()    -> Color { Color::Rgb(Rgb::new(0.90, 0.94, 0.91, None)) } // light sage
fn c_total_box()  -> Color { Color::Rgb(Rgb::new(0.94, 0.97, 0.95, None)) } // very pale sage
fn c_gray_mid()   -> Color { Color::Rgb(Rgb::new(0.70, 0.70, 0.70, None)) }
fn c_gray_light() -> Color { Color::Rgb(Rgb::new(0.88, 0.88, 0.88, None)) }
fn c_gray_text()  -> Color { Color::Rgb(Rgb::new(0.45, 0.45, 0.45, None)) }
fn c_label()      -> Color { Color::Rgb(Rgb::new(0.40, 0.40, 0.40, None)) }
fn c_navy_text()  -> Color { Color::Rgb(Rgb::new(0.08, 0.25, 0.17, None)) } // dark forest green

// ─── Utility helpers ──────────────────────────────────────────────────────────

fn money(n: f64) -> String {
    if !n.is_finite() { return "0.00".to_string(); }
    format!("{:.2}", n)
}

fn vat_rate_for(code: &str) -> f64 {
    match code.trim().to_uppercase().as_str() {
        "D" => 0.08,
        "E" => 0.18,
        _ => 0.0,
    }
}

fn vat_included(gross: f64, rate: f64) -> f64 {
    if rate <= 0.0 { return 0.0; }
    gross - (gross / (1.0 + rate))
}

fn clamp_text(s: &str, max: usize) -> String {
    let t = s.trim();
    if t.chars().count() <= max { return t.to_string(); }
    let mut out = String::new();
    for ch in t.chars().take(max.saturating_sub(3)) { out.push(ch); }
    out.push_str("...");
    out
}

/// Rough text width in mm (monospace approximation).
fn est_w(text: &str, size_pt: f32) -> f32 {
    text.chars().count() as f32 * size_pt * 0.17
}

/// Kosovo date: "2026-06-27" → "27.06.2026"
fn fmt_date(raw: &str) -> String {
    let s = raw.trim();
    if s.is_empty() || s == "-" { return "-".to_string(); }
    let date = s.split(|c: char| c == 'T' || c == ' ').next().unwrap_or(s);
    let p: Vec<&str> = date.split('-').collect();
    if p.len() == 3 && p[0].len() == 4 { return format!("{}.{}.{}", p[2], p[1], p[0]); }
    s.to_string()
}

fn opt(v: &Option<String>) -> &str {
    v.as_deref().map(str::trim).unwrap_or("")
}

// ─── Drawing primitives ───────────────────────────────────────────────────────

fn fill_rect(layer: &PdfLayerReference, x: f32, y_bot: f32, w: f32, h: f32, color: Color) {
    layer.save_graphics_state();
    layer.set_fill_color(color.clone());
    layer.set_outline_color(color);
    layer.set_outline_thickness(0.0_f32);
    layer.add_polygon(Polygon {
        rings: vec![vec![
            (Point::new(Mm(x),     Mm(y_bot)),     false),
            (Point::new(Mm(x + w), Mm(y_bot)),     false),
            (Point::new(Mm(x + w), Mm(y_bot + h)), false),
            (Point::new(Mm(x),     Mm(y_bot + h)), false),
        ]],
        mode: PaintMode::Fill,
        winding_order: WindingOrder::NonZero,
    });
    layer.restore_graphics_state();
}

fn hline(layer: &PdfLayerReference, x1: f32, x2: f32, y: f32, thickness: f32, color: Color) {
    layer.save_graphics_state();
    layer.set_outline_color(color);
    layer.set_outline_thickness(thickness);
    layer.add_line(Line {
        points: vec![
            (Point::new(Mm(x1), Mm(y)), false),
            (Point::new(Mm(x2), Mm(y)), false),
        ],
        is_closed: false,
    });
    layer.restore_graphics_state();
}

fn txt_l(layer: &PdfLayerReference, font: &IndirectFontRef, x: f32, y: f32, text: &str, sz: f32) {
    if !text.is_empty() {
        layer.use_text(text.to_string(), sz, Mm(x), Mm(y), font);
    }
}

fn txt_r(layer: &PdfLayerReference, font: &IndirectFontRef, x_r: f32, y: f32, text: &str, sz: f32) {
    if !text.is_empty() {
        let x = (x_r - est_w(text, sz)).max(ML);
        layer.use_text(text.to_string(), sz, Mm(x), Mm(y), font);
    }
}

fn txt_c(layer: &PdfLayerReference, font: &IndirectFontRef, xl: f32, xr: f32, y: f32, text: &str, sz: f32) {
    if !text.is_empty() {
        let x = (xl + (xr - xl - est_w(text, sz)) / 2.0).max(xl);
        layer.use_text(text.to_string(), sz, Mm(x), Mm(y), font);
    }
}

fn ctxt_l(layer: &PdfLayerReference, font: &IndirectFontRef, x: f32, y: f32, text: &str, sz: f32, color: Color) {
    layer.save_graphics_state();
    layer.set_fill_color(color);
    txt_l(layer, font, x, y, text, sz);
    layer.restore_graphics_state();
}

fn ctxt_r(layer: &PdfLayerReference, font: &IndirectFontRef, x_r: f32, y: f32, text: &str, sz: f32, color: Color) {
    layer.save_graphics_state();
    layer.set_fill_color(color);
    txt_r(layer, font, x_r, y, text, sz);
    layer.restore_graphics_state();
}

fn ctxt_c(layer: &PdfLayerReference, font: &IndirectFontRef, xl: f32, xr: f32, y: f32, text: &str, sz: f32, color: Color) {
    layer.save_graphics_state();
    layer.set_fill_color(color);
    txt_c(layer, font, xl, xr, y, text, sz);
    layer.restore_graphics_state();
}

// ─── Page overflow guard ──────────────────────────────────────────────────────

fn check_y(
    doc: &PdfDocumentReference,
    cur_page: &mut PdfPageIndex,
    layer: &mut PdfLayerReference,
    y: &mut f32,
    needed: f32,
) {
    if *y < needed + 20.0 {
        let (p, l) = doc.add_page(Mm(PAGE_W), Mm(PAGE_H), "Layer");
        *cur_page = p;
        *layer = doc.get_page(p).get_layer(l);
        *y = PAGE_H - 14.0;
    }
}

// ─── PNG placement helper ─────────────────────────────────────────────────────

fn place_png(layer: &PdfLayerReference, bytes: &[u8], x: f32, y_bot: f32, max_w: f32, max_h: f32) {
    let mut cur = Cursor::new(bytes);
    let decoder = match printpdf::image_crate::codecs::png::PngDecoder::new(&mut cur) {
        Ok(d) => d, Err(_) => return,
    };
    let img = match Image::try_from(decoder) {
        Ok(i) => i, Err(_) => return,
    };
    let w_px = img.image.width.0 as f32;
    let h_px = img.image.height.0 as f32;
    if w_px == 0.0 || h_px == 0.0 { return; }
    let mut dw = max_w;
    let mut dh = max_w * (h_px / w_px);
    if dh > max_h { dh = max_h; dw = max_h * (w_px / h_px); }
    let cx = x + (max_w - dw) / 2.0;
    let dpi = 300.0_f32;
    img.add_to_layer(layer.clone(), ImageTransform {
        translate_x: Some(Mm(cx)),
        translate_y: Some(Mm(y_bot)),
        rotate: None,
        scale_x: Some(dw * dpi / (w_px * 25.4)),
        scale_y: Some(dh * dpi / (h_px * 25.4)),
        dpi: Some(dpi),
    });
}

// ─── Shared header renderer ───────────────────────────────────────────────────

fn render_header(
    layer: &PdfLayerReference,
    font: &IndirectFontRef,
    font_b: &IndirectFontRef,
    title: &str,
    subtitle: &str,
    meta_line1: &str,
    meta_line2: &str,
    header_png: Option<&[u8]>,
    logo_png: Option<&[u8]>,
) -> f32 {
    if let Some(bytes) = header_png {
        let mut cur = Cursor::new(bytes);
        if let Ok(decoder) = printpdf::image_crate::codecs::png::PngDecoder::new(&mut cur) {
            if let Ok(img) = Image::try_from(decoder) {
                let w_px = img.image.width.0 as f32;
                let h_px = img.image.height.0 as f32;
                if w_px > 0.0 && h_px > 0.0 {
                    let aw = CW;
                    let nat_h = aw * (h_px / w_px);
                    let max_h = 55.0_f32;
                    let (dw, dh) = if nat_h <= max_h { (aw, nat_h) } else { (max_h * (w_px / h_px), max_h) };
                    let lx = ML + (aw - dw) / 2.0;
                    let img_y = PAGE_H - 10.0 - dh;
                    let dpi = 300.0_f32;
                    img.add_to_layer(layer.clone(), ImageTransform {
                        translate_x: Some(Mm(lx)),
                        translate_y: Some(Mm(img_y)),
                        rotate: None,
                        scale_x: Some(dw * dpi / (w_px * 25.4)),
                        scale_y: Some(dh * dpi / (h_px * 25.4)),
                        dpi: Some(dpi),
                    });
                    let bar_y = img_y - 10.0;
                    fill_rect(layer, 0.0, bar_y, PAGE_W, 10.0, c_navy());
                    ctxt_l(layer, font_b, ML + 2.0, bar_y + 3.0, title,      11.0, c_white());
                    ctxt_r(layer, font,   CR - 2.0, bar_y + 6.0, meta_line1, 8.0,  c_white());
                    ctxt_r(layer, font,   CR - 2.0, bar_y + 2.0, meta_line2, 8.0,  c_white());
                    hline(layer, 0.0, PAGE_W, bar_y, 1.5, c_navy_mid());
                    return bar_y - 6.0;
                }
            }
        }
    }

    // Solid navy header block
    let hdr_h   = 25.0_f32;
    let hdr_bot = PAGE_H - hdr_h;

    fill_rect(layer, 0.0, hdr_bot, PAGE_W, hdr_h, c_navy());

    if let Some(lb) = logo_png {
        place_png(layer, lb, CR - 28.0, hdr_bot + 2.5, 26.0, 18.0);
    }

    ctxt_l(layer, font_b, ML + 2.0, hdr_bot + 16.0, title,    16.0, c_white());
    if !subtitle.is_empty() {
        ctxt_l(layer, font, ML + 2.0, hdr_bot + 9.0, subtitle, 8.5, c_navy_pale());
    }
    ctxt_r(layer, font_b, CR - 2.0, hdr_bot + 16.5, meta_line1, 9.0, c_white());
    ctxt_r(layer, font,   CR - 2.0, hdr_bot + 9.0,  meta_line2, 8.5, c_white());

    hline(layer, 0.0, PAGE_W, hdr_bot, 1.5, c_navy_mid());

    hdr_bot - 6.0
}

// ─── Label + value row helper ─────────────────────────────────────────────────

fn info_row(layer: &PdfLayerReference, font: &IndirectFontRef, x: f32, y: f32, label: &str, value: &str, lw: f32) {
    ctxt_l(layer, font, x,      y, &format!("{}:", label), 8.5, c_label());
    txt_l (layer, font, x + lw, y, value,                  8.5);
}

// ─── Invoice PDF ──────────────────────────────────────────────────────────────

pub fn render_invoice_pdf(data: &InvoicePdfData) -> anyhow::Result<Vec<u8>> {
    let (doc, page1, layer1) = PdfDocument::new("Fature", Mm(PAGE_W), Mm(PAGE_H), "Layer 1");
    let font   = doc.add_builtin_font(BuiltinFont::Helvetica).context("font")?;
    let font_b = doc.add_builtin_font(BuiltinFont::HelveticaBold).context("font bold")?;

    let mut page  = page1;
    let mut layer = doc.get_page(page).get_layer(layer1);

    let date_str = fmt_date(opt(&data.date));
    let id_disp  = clamp_text(&data.invoice_id, 16);

    let mut y = render_header(
        &layer, &font, &font_b,
        "FATURE", &data.clinic_name,
        &format!("Nr. {}", id_disp),
        &format!("Data: {}", date_str),
        data.header_png.as_deref(), data.logo_png.as_deref(),
    );

    // ── Two-column info section ────────────────────────────────────────────
    let mid = ML + CW * 0.52;

    ctxt_l(&layer, &font_b, ML,  y, "Te dhenat e pacientit", 9.5, c_navy_text());
    ctxt_l(&layer, &font_b, mid, y, "Detajet e fatures",    9.5, c_navy_text());
    y -= LH;

    let mut left_y  = y;
    let mut right_y = y;

    // Left: patient info
    {
        let addr = opt(&data.client_address);
        let city = opt(&data.client_city);
        let combined = if addr.is_empty() { city.to_string() }
            else if city.is_empty() { addr.to_string() }
            else { format!("{}, {}", addr, city) };
        if !combined.is_empty() {
            info_row(&layer, &font, ML, left_y, "Adresa", &clamp_text(&combined, 34), 17.0);
            left_y -= LH - 0.5;
        }
    }
    for (label, value) in &[
        ("Emri",   data.client_name.as_str()),
        ("Kodi",   opt(&data.client_code)),
        ("Lindje", &fmt_date(opt(&data.client_dob))),
        ("Tel",    opt(&data.client_phone)),
        ("Email",  opt(&data.client_email)),
    ] {
        if value.is_empty() || *value == "-" { continue; }
        info_row(&layer, &font, ML, left_y, label, value, 17.0);
        left_y -= LH - 0.5;
    }
    if !opt(&data.notes).is_empty() {
        info_row(&layer, &font, ML, left_y, "Shenime", &clamp_text(opt(&data.notes), 32), 17.0);
        left_y -= LH - 0.5;
    }

    // Right: invoice meta
    for (label, value) in &[
        ("Nr. Fatures", id_disp.as_str()),
        ("Data",        date_str.as_str()),
        ("Klinika",     data.clinic_name.as_str()),
    ] {
        info_row(&layer, &font, mid, right_y, label, value, 23.0);
        right_y -= LH - 0.5;
    }

    y = left_y.min(right_y) - 5.0;

    // Navy divider
    hline(&layer, ML, CR, y, 1.2, c_navy());
    y -= 8.0;

    // ── Table header row ───────────────────────────────────────────────────
    let row_h = 7.5_f32;
    fill_rect(&layer, ML, y, CW, row_h, c_hdr_row());
    hline(&layer, ML, CR, y + row_h, 0.5, c_gray_mid());

    let th_y = y + 2.2;
    ctxt_r(&layer, &font_b, T_NR_R,    th_y, "Nr",                      8.5, c_navy_text());
    ctxt_l(&layer, &font_b, T_DESC_L,  th_y, "Sherbimi / Pershkrimi",   8.5, c_navy_text());
    ctxt_r(&layer, &font_b, T_QTY_R,   th_y, "Sasia",                   8.5, c_navy_text());
    ctxt_r(&layer, &font_b, T_PRICE_R, th_y, "Cmimi",                   8.5, c_navy_text());
    ctxt_c(&layer, &font_b, T_VAT_L, T_VAT_R, th_y, "TVSH",            8.5, c_navy_text());
    ctxt_r(&layer, &font_b, T_TOT_R,   th_y, "Totali",                  8.5, c_navy_text());

    y -= 1.5;

    // ── Table rows ─────────────────────────────────────────────────────────
    let mut subtotal = 0.0_f64;
    let mut vat8     = 0.0_f64;
    let mut vat18    = 0.0_f64;

    if data.lines.is_empty() {
        check_y(&doc, &mut page, &mut layer, &mut y, LH + 4.0);
        y -= LH;
        ctxt_l(&layer, &font, T_DESC_L, y, "(pa procedura)", 9.0, c_gray_text());
        y -= 2.0;
    } else {
        for (idx, ln) in data.lines.iter().enumerate() {
            check_y(&doc, &mut page, &mut layer, &mut y, LH + 4.0);
            y -= LH;

            let tooth = ln.tooth.as_deref().unwrap_or("").trim();
            let desc = if tooth.is_empty() {
                clamp_text(&ln.title, 50)
            } else {
                clamp_text(&format!("Dh.{} - {}", tooth, ln.title), 50)
            };
            let vat_code = ln.vat_code.trim().to_uppercase();
            let sub  = ln.qty * ln.unit_price;
            subtotal += sub;
            let rate = vat_rate_for(&vat_code);
            let vat  = vat_included(sub, rate);
            if (rate - 0.08).abs() < 1e-7 { vat8  += vat; }
            if (rate - 0.18).abs() < 1e-7 { vat18 += vat; }

            if idx % 2 == 1 {
                fill_rect(&layer, ML, y - 1.5, CW, LH, c_row_alt());
            }

            txt_r(&layer, &font, T_NR_R,    y, &(idx + 1).to_string(),           9.0);
            txt_l(&layer, &font, T_DESC_L,  y, &desc,                            9.0);
            txt_r(&layer, &font, T_QTY_R,   y, &money(ln.qty),                   9.0);
            txt_r(&layer, &font, T_PRICE_R, y, &money(ln.unit_price),            9.0);
            txt_c(&layer, &font, T_VAT_L, T_VAT_R, y, &vat_code,                9.0);
            txt_r(&layer, &font, T_TOT_R,   y, &format!("{} EUR", money(sub)),   9.0);

            hline(&layer, ML, CR, y - 2.0, 0.2, c_gray_light());
        }
    }

    y -= 5.0;
    check_y(&doc, &mut page, &mut layer, &mut y, 45.0);

    // ── Totals section ─────────────────────────────────────────────────────
    hline(&layer, ML, CR, y, 1.2, c_navy());
    y -= LH + 1.0;

    if vat8 > 0.0 {
        ctxt_l(&layer, &font, ML, y, "TVSH 8% e perfshire:", 8.5, c_gray_text());
        ctxt_r(&layer, &font, CR, y, &format!("{} EUR", money(vat8)), 8.5, c_gray_text());
        y -= LH - 0.5;
    }
    if vat18 > 0.0 {
        ctxt_l(&layer, &font, ML, y, "TVSH 18% e perfshire:", 8.5, c_gray_text());
        ctxt_r(&layer, &font, CR, y, &format!("{} EUR", money(vat18)), 8.5, c_gray_text());
        y -= LH - 0.5;
    }
    if vat8 > 0.0 || vat18 > 0.0 {
        let net = subtotal - vat8 - vat18;
        ctxt_l(&layer, &font, ML, y, "Nentotali (pa TVSH):", 8.5, c_gray_text());
        ctxt_r(&layer, &font, CR, y, &format!("{} EUR", money(net)), 8.5, c_gray_text());
        y -= 3.0;
        hline(&layer, ML, CR, y, 0.4, c_gray_mid());
        y -= 3.0;
    }

    // Total highlight box
    let box_h = 11.0_f32;
    fill_rect(&layer, ML, y - box_h + 3.5, CW, box_h, c_navy());
    ctxt_l(&layer, &font_b, ML + 4.0, y - box_h + 7.0, "TOTALI PER PAGESE",          10.5, c_white());
    ctxt_r(&layer, &font_b, CR - 4.0, y - box_h + 6.5, &format!("{} EUR", money(data.total)), 12.5, c_white());
    y -= box_h + 2.0;

    // Fiscal/non-fiscal note
    if data.fiscal_total > 0.0 && data.non_fiscal_total > 0.0 {
        check_y(&doc, &mut page, &mut layer, &mut y, LH + 2.0);
        y -= LH;
        ctxt_l(&layer, &font, ML, y,
            &format!("Fiskal: {} EUR    |    Jo-fiskal: {} EUR",
                money(data.fiscal_total), money(data.non_fiscal_total)),
            8.0, c_gray_text());
    }

    // Bank account / payment info
    if let Some(ref ba) = data.bank_account {
        let ba = ba.trim();
        if !ba.is_empty() {
            check_y(&doc, &mut page, &mut layer, &mut y, LH + 2.0);
            y -= LH;
            ctxt_l(&layer, &font_b, ML, y, "Xhirollogaria:", 8.5, c_label());
            ctxt_l(&layer, &font, ML + 29.0, y, ba, 8.5, c_gray_text());
        }
    }

    // ── Footer ─────────────────────────────────────────────────────────────
    hline(&layer, ML, CR, 17.5, 0.4, c_gray_light());
    ctxt_c(&layer, &font, ML, CR, 12.0,
        &format!("Mjeku  |  {}  |  PROGRAMERI MJEKU  www.programeri.net", date_str),
        7.5, c_gray_text());

    save_pdf(doc)
}

// ─── Visit PDF ────────────────────────────────────────────────────────────────

pub fn render_visit_pdf(data: &VisitPdfData) -> anyhow::Result<Vec<u8>> {
    let (doc, page1, layer1) = PdfDocument::new("Vizite", Mm(PAGE_W), Mm(PAGE_H), "Layer 1");
    let font   = doc.add_builtin_font(BuiltinFont::Helvetica).context("font")?;
    let font_b = doc.add_builtin_font(BuiltinFont::HelveticaBold).context("font bold")?;

    let mut page  = page1;
    let mut layer = doc.get_page(page).get_layer(layer1);

    let date_str = fmt_date(opt(&data.date));
    let time_str = opt(&data.visit_time).to_string();
    let id_disp  = clamp_text(&data.visit_id, 14);
    let meta2    = if time_str.is_empty() {
        format!("Data: {}", date_str)
    } else {
        format!("Data: {}  {}", date_str, time_str)
    };
    let doc_name = opt(&data.doctor_name);
    let subtitle = if doc_name.is_empty() {
        data.clinic_name.clone()
    } else {
        format!("{}  |  Dr. {}", data.clinic_name, doc_name)
    };

    let mut y = render_header(
        &layer, &font, &font_b,
        "RAPORT VIZITE", &subtitle,
        &format!("Nr. {}", id_disp), &meta2,
        data.header_png.as_deref(), data.logo_png.as_deref(),
    );

    // ── Two-column: patient + visit meta ───────────────────────────────────
    let mid = ML + CW * 0.52;

    ctxt_l(&layer, &font_b, ML,  y, "Te dhenat e pacientit", 9.5, c_navy_text());
    ctxt_l(&layer, &font_b, mid, y, "Detajet e vizites",     9.5, c_navy_text());
    y -= LH;

    let mut left_y  = y;
    let mut right_y = y;

    {
        let addr = opt(&data.client_address);
        let city = opt(&data.client_city);
        let combined = if addr.is_empty() { city.to_string() }
            else if city.is_empty() { addr.to_string() }
            else { format!("{}, {}", addr, city) };
        if !combined.is_empty() {
            info_row(&layer, &font, ML, left_y, "Adresa", &clamp_text(&combined, 32), 17.0);
            left_y -= LH - 0.5;
        }
    }
    for (label, value) in &[
        ("Emri",   data.client_name.as_str()),
        ("Kodi",   opt(&data.client_code)),
        ("Lindje", &fmt_date(opt(&data.client_dob))),
        ("Tel",    opt(&data.client_phone)),
        ("Email",  opt(&data.client_email)),
    ] {
        if value.is_empty() || *value == "-" { continue; }
        info_row(&layer, &font, ML, left_y, label, value, 17.0);
        left_y -= LH - 0.5;
    }

    for (label, value) in &[
        ("Nr. Vizites", id_disp.as_str()),
        ("Data",        date_str.as_str()),
        ("Ora",         time_str.as_str()),
        ("Statusi",     data.status.as_str()),
        ("Mjeku",       opt(&data.doctor_name)),
        ("Klinika",     data.clinic_name.as_str()),
    ] {
        if value.is_empty() { continue; }
        info_row(&layer, &font, mid, right_y, label, value, 23.0);
        right_y -= LH - 0.5;
    }

    y = left_y.min(right_y) - 5.0;

    // ── Vital signs ────────────────────────────────────────────────────────
    let mut vital_pairs: Vec<(String, String)> = Vec::new();
    for (label, val_opt, unit_opt) in &[
        ("Pesha",             &data.body_weight,        &data.body_weight_unit),
        ("Gjatesia",          &data.body_height,        &data.body_height_unit),
        ("Perimetri i kokes", &data.head_circumference, &data.head_circumference_unit),
        ("Temperatura",       &data.body_temperature,   &data.body_temperature_unit),
        ("Oksigjeni",         &data.blood_oxygen,       &data.blood_oxygen_unit),
        ("Glicemia",          &data.glycemia,           &data.glycemia_unit),
        ("Pulsi",             &data.pulse,              &data.pulse_unit),
    ] {
        let v = opt(val_opt).trim();
        if v.is_empty() { continue; }
        let u = opt(unit_opt).trim();
        vital_pairs.push((label.to_string(), if u.is_empty() { v.to_string() } else { format!("{} {}", v, u) }));
    }
    if let Some(bmi) = &data.bmi { let b = bmi.trim(); if !b.is_empty() { vital_pairs.push(("BMI".to_string(), b.to_string())); } }
    {
        let bp_s = opt(&data.blood_pressure_systolic);
        let bp_d = opt(&data.blood_pressure_diastolic);
        if !bp_s.is_empty() || !bp_d.is_empty() {
            let mut v = format!("{}/{}", if bp_s.is_empty() { "-" } else { bp_s }, if bp_d.is_empty() { "-" } else { bp_d });
            let u = opt(&data.blood_pressure_unit);
            if !u.is_empty() { v.push(' '); v.push_str(u); }
            vital_pairs.push(("Tensioni arterial".to_string(), v));
        }
    }

    if !vital_pairs.is_empty() {
        check_y(&doc, &mut page, &mut layer, &mut y, LH * 3.0);
        hline(&layer, ML, CR, y, 1.2, c_navy());
        y -= LH + 1.0;
        ctxt_l(&layer, &font_b, ML, y, "Parametrat klinike", 9.5, c_navy_text());
        y -= LH;

        let col_x = [ML, mid];
        let mut ys = [y, y];
        let mut col = 0usize;
        for (label, value) in &vital_pairs {
            check_y(&doc, &mut page, &mut layer, &mut ys[col], LH);
            ctxt_l(&layer, &font, col_x[col],        ys[col], &format!("{}:", label), 8.5, c_label());
            txt_l (&layer, &font, col_x[col] + 28.0, ys[col], value, 8.5);
            ys[col] -= LH - 0.5;
            col = 1 - col;
        }
        y = ys[0].min(ys[1]) - 3.0;
    }

    // ── Clinical text sections ─────────────────────────────────────────────
    let sections: Vec<(&str, &Option<String>)> = vec![
        ("Ankesat",          &data.complaints),
        ("Shenime shtese",   &data.additional_notes),
        ("Kontrollat",       &data.controls),
        ("Verejtjet",        &data.remarks),
        ("Analizat",         &data.analyses),
        ("Keshillat",        &data.advice),
        ("Terapite",         &data.therapies),
        ("Diagnoza",         &data.diagnosis),
        ("Ekzaminimet",      &data.examinations),
        ("Shenime vizite",   &data.notes),
    ];

    let has_sections = sections.iter().any(|(_, v)| !opt(v).is_empty());

    if has_sections {
        check_y(&doc, &mut page, &mut layer, &mut y, LH * 2.0);
        hline(&layer, ML, CR, y, 1.2, c_navy());
        y -= LH + 1.0;
        ctxt_l(&layer, &font_b, ML, y, "Informacioni klinik", 9.5, c_navy_text());
        y -= LH;
    }

    for (title, content) in &sections {
        let text = opt(content);
        if text.is_empty() { continue; }

        check_y(&doc, &mut page, &mut layer, &mut y, LH * 2.5);
        fill_rect(&layer, ML, y - 1.2, CW, LH, c_total_box());
        ctxt_l(&layer, &font_b, ML + 2.0, y + 0.8, title, 9.0, c_navy_text());
        y -= LH + 1.0;

        for line in text.lines() {
            let t = line.trim();
            if t.is_empty() { continue; }
            check_y(&doc, &mut page, &mut layer, &mut y, LH);
            txt_l(&layer, &font, ML + 3.0, y, t, 9.0);
            y -= LH - 0.5;
        }
        y -= 2.0;
    }

    // ── Procedures table ───────────────────────────────────────────────────
    if !data.lines.is_empty() {
        check_y(&doc, &mut page, &mut layer, &mut y, LH * 5.0);
        hline(&layer, ML, CR, y, 1.2, c_navy());
        y -= LH + 1.0;
        ctxt_l(&layer, &font_b, ML, y, "Procedurat e vizites", 9.5, c_navy_text());
        y -= LH + 1.0;

        let row_h = 7.5_f32;
        fill_rect(&layer, ML, y, CW, row_h, c_hdr_row());
        hline(&layer, ML, CR, y + row_h, 0.5, c_gray_mid());
        let th_y = y + 2.2;
        ctxt_r(&layer, &font_b, T_NR_R,    th_y, "Nr",        8.5, c_navy_text());
        ctxt_l(&layer, &font_b, T_DESC_L,  th_y, "Procedura", 8.5, c_navy_text());
        ctxt_r(&layer, &font_b, T_QTY_R,   th_y, "Sasia",     8.5, c_navy_text());
        ctxt_r(&layer, &font_b, T_PRICE_R, th_y, "Cmimi",     8.5, c_navy_text());
        ctxt_c(&layer, &font_b, T_VAT_L, T_VAT_R, th_y, "Fiskal", 8.5, c_navy_text());
        ctxt_r(&layer, &font_b, T_TOT_R,   th_y, "Totali",    8.5, c_navy_text());
        y -= 1.5;

        let mut total = 0.0_f64;
        for (idx, ln) in data.lines.iter().enumerate() {
            check_y(&doc, &mut page, &mut layer, &mut y, LH + 4.0);
            y -= LH;
            let tooth = ln.tooth.as_deref().unwrap_or("").trim();
            let desc = if tooth.is_empty() {
                clamp_text(&ln.title, 50)
            } else {
                clamp_text(&format!("Dh.{} - {}", tooth, ln.title), 50)
            };
            let sub = ln.qty * ln.unit_price;
            total += sub;
            if idx % 2 == 1 { fill_rect(&layer, ML, y - 1.5, CW, LH, c_row_alt()); }
            txt_r(&layer, &font, T_NR_R,    y, &(idx + 1).to_string(),            9.0);
            txt_l(&layer, &font, T_DESC_L,  y, &desc,                             9.0);
            txt_r(&layer, &font, T_QTY_R,   y, &money(ln.qty),                    9.0);
            txt_r(&layer, &font, T_PRICE_R, y, &money(ln.unit_price),             9.0);
            txt_c(&layer, &font, T_VAT_L, T_VAT_R, y, if ln.fiscal { "Po" } else { "Jo" }, 9.0);
            txt_r(&layer, &font, T_TOT_R,   y, &format!("{} EUR", money(sub)),    9.0);
            hline(&layer, ML, CR, y - 2.0, 0.2, c_gray_light());
        }

        y -= 5.0;
        check_y(&doc, &mut page, &mut layer, &mut y, 20.0);
        hline(&layer, ML, CR, y, 1.2, c_navy());
        y -= LH + 1.0;

        let box_h = 11.0_f32;
        fill_rect(&layer, ML, y - box_h + 3.5, CW, box_h, c_navy());
        ctxt_l(&layer, &font_b, ML + 4.0, y - box_h + 7.0, "TOTALI I PROCEDURAVE",         10.0, c_white());
        ctxt_r(&layer, &font_b, CR - 4.0, y - box_h + 6.5, &format!("{} EUR", money(total)), 12.5, c_white());
    }

    // ── Footer ─────────────────────────────────────────────────────────────
    hline(&layer, ML, CR, 17.5, 0.4, c_gray_light());
    ctxt_c(&layer, &font, ML, CR, 12.0,
        &format!("Mjeku  |  {}  |  PROGRAMERI MJEKU  www.programeri.net", date_str),
        7.5, c_gray_text());

    save_pdf(doc)
}

// ─── Offer PDF ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct OfferPdfData {
    pub clinic_name: String,
    pub clinic_address: Option<String>,
    pub clinic_phone: Option<String>,
    pub header_png: Option<Vec<u8>>,
    pub logo_png: Option<Vec<u8>>,
    pub offer_number: String,
    pub date: String,
    pub valid_until: Option<String>,
    pub client_name: String,
    pub client_phone: Option<String>,
    pub client_email: Option<String>,
    pub notes: Option<String>,
    pub lines: Vec<OfferPdfLine>,
    pub vat_pct: f64,
    pub subtotal: f64,
    pub vat_amount: f64,
    pub total: f64,
}

#[derive(Debug, Clone)]
pub struct OfferPdfLine {
    pub description: String,
    pub qty: f64,
    pub unit_price: f64,
    pub discount_pct: f64,
    pub line_total: f64,
}

// Offer table columns (mm from page left)
const O_NR_R: f32    = 23.0;
const O_DESC_L: f32  = 25.0;
const O_QTY_R: f32   = 115.0;
const O_PRICE_R: f32 = 140.0;
const O_DISC_R: f32  = 158.0;
const O_TOT_R: f32   = 196.0;

pub fn generate_offer_pdf(data: &OfferPdfData) -> anyhow::Result<Vec<u8>> {
    let (doc, page1, layer1) = PdfDocument::new("Oferte", Mm(PAGE_W), Mm(PAGE_H), "Layer 1");
    let font   = doc.add_builtin_font(BuiltinFont::Helvetica).context("font")?;
    let font_b = doc.add_builtin_font(BuiltinFont::HelveticaBold).context("font bold")?;

    let mut page  = page1;
    let mut layer = doc.get_page(page).get_layer(layer1);

    let date_str    = fmt_date(&data.date);
    let valid_str   = data.valid_until.as_deref().map(fmt_date).unwrap_or_else(|| "-".to_string());
    let nr_display  = clamp_text(&data.offer_number, 20);

    let mut y = render_header(
        &layer, &font, &font_b,
        "OFERTE", &data.clinic_name,
        &format!("Nr. {}", nr_display),
        &format!("Data: {}", date_str),
        data.header_png.as_deref(), data.logo_png.as_deref(),
    );

    // ── Clinic address / phone row ─────────────────────────────────────────
    let addr = opt(&data.clinic_address);
    let phone = opt(&data.clinic_phone);
    if !addr.is_empty() || !phone.is_empty() {
        let combined = if !addr.is_empty() && !phone.is_empty() {
            format!("{}  |  Tel: {}", addr, phone)
        } else if !addr.is_empty() {
            addr.to_string()
        } else {
            format!("Tel: {}", phone)
        };
        ctxt_l(&layer, &font, ML, y, &clamp_text(&combined, 60), 8.0, c_gray_text());
        y -= LH;
    }

    // ── Two-column: client + offer meta ───────────────────────────────────
    let mid = ML + CW * 0.52;

    ctxt_l(&layer, &font_b, ML,  y, "Te dhenat e klientit", 9.5, c_navy_text());
    ctxt_l(&layer, &font_b, mid, y, "Detajet e ofertes",    9.5, c_navy_text());
    y -= LH;

    let mut left_y  = y;
    let mut right_y = y;

    // Left: client info
    for (label, value) in &[
        ("Emri",  data.client_name.as_str()),
        ("Tel",   opt(&data.client_phone)),
        ("Email", opt(&data.client_email)),
    ] {
        if value.is_empty() { continue; }
        info_row(&layer, &font, ML, left_y, label, value, 17.0);
        left_y -= LH - 0.5;
    }

    // Right: offer meta
    for (label, value) in &[
        ("Nr. Ofertes", nr_display.as_str()),
        ("Data",        date_str.as_str()),
        ("Vlefshmeria", valid_str.as_str()),
        ("Klinika",     data.clinic_name.as_str()),
    ] {
        if value.is_empty() { continue; }
        info_row(&layer, &font, mid, right_y, label, value, 23.0);
        right_y -= LH - 0.5;
    }

    y = left_y.min(right_y) - 5.0;

    // Notes
    if !opt(&data.notes).is_empty() {
        ctxt_l(&layer, &font, ML, y, &format!("Shenime: {}", clamp_text(opt(&data.notes), 70)), 8.5, c_gray_text());
        y -= LH;
    }

    // Navy divider
    hline(&layer, ML, CR, y, 1.2, c_navy());
    y -= 8.0;

    // ── Table header row ───────────────────────────────────────────────────
    let row_h = 7.5_f32;
    fill_rect(&layer, ML, y, CW, row_h, c_hdr_row());
    hline(&layer, ML, CR, y + row_h, 0.5, c_gray_mid());

    let th_y = y + 2.2;
    ctxt_r(&layer, &font_b, O_NR_R,    th_y, "Nr",               8.5, c_navy_text());
    ctxt_l(&layer, &font_b, O_DESC_L,  th_y, "Sherbimi",          8.5, c_navy_text());
    ctxt_r(&layer, &font_b, O_QTY_R,   th_y, "Sasia",             8.5, c_navy_text());
    ctxt_r(&layer, &font_b, O_PRICE_R, th_y, "Cmimi/njesi",       8.5, c_navy_text());
    ctxt_r(&layer, &font_b, O_DISC_R,  th_y, "Zb.%",              8.5, c_navy_text());
    ctxt_r(&layer, &font_b, O_TOT_R,   th_y, "Totali",            8.5, c_navy_text());

    y -= 1.5;

    // ── Table rows ─────────────────────────────────────────────────────────
    if data.lines.is_empty() {
        check_y(&doc, &mut page, &mut layer, &mut y, LH + 4.0);
        y -= LH;
        ctxt_l(&layer, &font, O_DESC_L, y, "(pa sherbime)", 9.0, c_gray_text());
        y -= 2.0;
    } else {
        for (idx, ln) in data.lines.iter().enumerate() {
            check_y(&doc, &mut page, &mut layer, &mut y, LH + 4.0);
            y -= LH;

            let desc = clamp_text(&ln.description, 45);

            if idx % 2 == 1 {
                fill_rect(&layer, ML, y - 1.5, CW, LH, c_row_alt());
            }

            txt_r(&layer, &font, O_NR_R,    y, &(idx + 1).to_string(),               9.0);
            txt_l(&layer, &font, O_DESC_L,  y, &desc,                                9.0);
            txt_r(&layer, &font, O_QTY_R,   y, &money(ln.qty),                       9.0);
            txt_r(&layer, &font, O_PRICE_R, y, &money(ln.unit_price),                9.0);
            txt_r(&layer, &font, O_DISC_R,  y, &format!("{}%", money(ln.discount_pct)), 9.0);
            txt_r(&layer, &font, O_TOT_R,   y, &format!("{} EUR", money(ln.line_total)), 9.0);

            hline(&layer, ML, CR, y - 2.0, 0.2, c_gray_light());
        }
    }

    y -= 5.0;
    check_y(&doc, &mut page, &mut layer, &mut y, 50.0);

    // ── Totals section ─────────────────────────────────────────────────────
    hline(&layer, ML, CR, y, 1.2, c_navy());
    y -= LH + 1.0;

    // Subtotal line
    ctxt_l(&layer, &font, ML, y, "Nentotali:", 8.5, c_gray_text());
    ctxt_r(&layer, &font, CR, y, &format!("{} EUR", money(data.subtotal)), 8.5, c_gray_text());
    y -= LH - 0.5;

    // VAT line (only if > 0)
    if data.vat_amount > 0.0 {
        ctxt_l(&layer, &font, ML, y, &format!("TVSH ({}%):", money(data.vat_pct)), 8.5, c_gray_text());
        ctxt_r(&layer, &font, CR, y, &format!("{} EUR", money(data.vat_amount)), 8.5, c_gray_text());
        y -= 3.0;
        hline(&layer, ML, CR, y, 0.4, c_gray_mid());
        y -= 3.0;
    }

    // Total highlight box
    let box_h = 11.0_f32;
    fill_rect(&layer, ML, y - box_h + 3.5, CW, box_h, c_navy());
    ctxt_l(&layer, &font_b, ML + 4.0, y - box_h + 7.0, "TOTALI",                              10.5, c_white());
    ctxt_r(&layer, &font_b, CR - 4.0, y - box_h + 6.5, &format!("{} EUR", money(data.total)), 12.5, c_white());
    y -= box_h + 4.0;

    // Validity note footer
    if data.valid_until.is_some() {
        check_y(&doc, &mut page, &mut layer, &mut y, LH + 2.0);
        y -= LH;
        ctxt_l(&layer, &font, ML, y,
            &format!("* Kjo oferte vlen deri me {}", valid_str),
            8.0, c_gray_text());
    }

    // ── Footer ─────────────────────────────────────────────────────────────
    hline(&layer, ML, CR, 17.5, 0.4, c_gray_light());
    ctxt_c(&layer, &font, ML, CR, 12.0,
        &format!("Mjeku  |  {}  |  PROGRAMERI MJEKU  www.programeri.net", date_str),
        7.5, c_gray_text());

    save_pdf(doc)
}

// ─── PDF serializer ───────────────────────────────────────────────────────────

fn save_pdf(doc: printpdf::PdfDocumentReference) -> anyhow::Result<Vec<u8>> {
    let mut writer = BufWriter::new(Cursor::new(Vec::<u8>::new()));
    doc.save(&mut writer).map_err(|e| anyhow!("save pdf: {e}"))?;
    let cursor = writer.into_inner().map_err(|e| anyhow!("flush: {e}"))?;
    Ok(cursor.into_inner())
}

// ─── Sample generation (tests only) ──────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_sample_invoice_pdf() {
        let data = InvoicePdfData {
            clinic_name: "Klinika Dentare Smile".to_string(),
            header_png: None,
            logo_png: None,
            invoice_id: "F-2026-0042".to_string(),
            date: Some("2026-06-27".to_string()),
            client_name: "Gentiana Berisha".to_string(),
            client_code: Some("KB-00142".to_string()),
            client_dob: Some("1985-03-14".to_string()),
            client_address: Some("Rr. Nene Tereza, Nr. 24".to_string()),
            client_city: Some("Prishtine".to_string()),
            client_phone: Some("+383 44 123 456".to_string()),
            client_email: Some("gberisha@email.com".to_string()),
            notes: None,
            bank_account: Some("NL91 ABNA 0417 1643 00".to_string()),
            lines: vec![
                InvoiceLine {
                    tooth: Some("11".to_string()),
                    title: "Ekzaminim dhe konsulte dentare".to_string(),
                    qty: 1.0, unit_price: 20.0, fiscal: true, vat_code: "C".to_string(),
                },
                InvoiceLine {
                    tooth: Some("21".to_string()),
                    title: "Mbushje kompozite klase II".to_string(),
                    qty: 1.0, unit_price: 75.0, fiscal: true, vat_code: "C".to_string(),
                },
                InvoiceLine {
                    tooth: Some("22".to_string()),
                    title: "Mbushje kompozite klase II".to_string(),
                    qty: 1.0, unit_price: 75.0, fiscal: true, vat_code: "C".to_string(),
                },
                InvoiceLine {
                    tooth: None,
                    title: "Radiografi periapikale".to_string(),
                    qty: 2.0, unit_price: 15.0, fiscal: true, vat_code: "E".to_string(),
                },
                InvoiceLine {
                    tooth: None,
                    title: "Pastrim profesional (profilaksi)".to_string(),
                    qty: 1.0, unit_price: 40.0, fiscal: false, vat_code: "A".to_string(),
                },
            ],
            total: 225.0,
            fiscal_total: 185.0,
            non_fiscal_total: 40.0,
        };
        let pdf = render_invoice_pdf(&data).expect("render invoice");
        let path = "C:/Users/Fatlind Mazreku/Desktop/sample_fature.pdf";
        std::fs::write(path, &pdf).expect("write pdf");
        println!("Saved {} bytes to {}", pdf.len(), path);
    }

    #[test]
    fn generate_sample_visit_pdf() {
        let data = VisitPdfData {
            clinic_name: "Klinika Dentare Smile".to_string(),
            header_png: None,
            logo_png: None,
            visit_id: "V-2026-0198".to_string(),
            date: Some("2026-06-27".to_string()),
            visit_time: Some("10:30".to_string()),
            status: "Final".to_string(),
            doctor_name: Some("Artan Krasniqi".to_string()),
            client_name: "Gentiana Berisha".to_string(),
            client_code: Some("KB-00142".to_string()),
            client_dob: Some("1985-03-14".to_string()),
            client_address: Some("Rr. Nene Tereza, Nr. 24".to_string()),
            client_city: Some("Prishtine".to_string()),
            client_phone: Some("+383 44 123 456".to_string()),
            client_email: Some("gberisha@email.com".to_string()),
            notes: None,
            body_weight: Some("68".to_string()),
            body_weight_unit: Some("kg".to_string()),
            body_height: Some("167".to_string()),
            body_height_unit: Some("cm".to_string()),
            head_circumference: None, head_circumference_unit: None,
            body_temperature: Some("36.7".to_string()),
            body_temperature_unit: Some("C".to_string()),
            blood_oxygen: Some("98".to_string()),
            blood_oxygen_unit: Some("%".to_string()),
            glycemia: None, glycemia_unit: None,
            pulse: Some("72".to_string()),
            pulse_unit: Some("bpm".to_string()),
            bmi: Some("24.4".to_string()),
            blood_pressure_systolic: Some("120".to_string()),
            blood_pressure_diastolic: Some("80".to_string()),
            blood_pressure_unit: Some("mmHg".to_string()),
            complaints: Some("Pacientja ankohet per dhembje te lehte ne dhembin 21 gjate kafshimit. Dhembja ka filluar para 5 ditesh.".to_string()),
            additional_notes: Some("Historia dentare: mbushje te medha ne sektoren posteriore. Pacientja nuk ka alergji te njohura ndaj anestetikeve.".to_string()),
            controls: Some("Kontroll i rekomandueshme pas 2 javesh.".to_string()),
            remarks: None,
            analyses: None,
            advice: Some("Evitoni ushqimet e ftohta dhe te ngrohta per 48 ore. Perdorni paste me fluoride.".to_string()),
            therapies: Some("Aplikuar anestezi lokale. Heqja e mbushjes se vjeter dhe ri-mbushja me kompozit.".to_string()),
            diagnosis: Some("Karies sekondar dhembit 21, klase II sipas Black.".to_string()),
            examinations: Some("Ekzaminim klinik dhe radiografik. Testi i vitalitetit pozitiv.".to_string()),
            lines: vec![
                InvoiceLine {
                    tooth: Some("21".to_string()),
                    title: "Mbushje kompozite klase II".to_string(),
                    qty: 1.0, unit_price: 75.0, fiscal: true, vat_code: "C".to_string(),
                },
                InvoiceLine {
                    tooth: None,
                    title: "Radiografi periapikale".to_string(),
                    qty: 1.0, unit_price: 15.0, fiscal: true, vat_code: "E".to_string(),
                },
            ],
            total: 90.0,
        };
        let pdf = render_visit_pdf(&data).expect("render visit");
        let path = "C:/Users/Fatlind Mazreku/Desktop/sample_vizite.pdf";
        std::fs::write(path, &pdf).expect("write pdf");
        println!("Saved {} bytes to {}", pdf.len(), path);
    }
}

// ─── Receta mjekesore ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct PrescriptionPdfData {
    pub clinic_name: String,
    pub header_png: Option<Vec<u8>>,
    pub logo_png: Option<Vec<u8>>,
    pub date: String, // YYYY-MM-DD
    pub doctor_name: Option<String>,
    pub doctor_title: Option<String>,
    pub client_name: Option<String>, // None => recete e zbrazet
    pub client_dob: Option<String>,
    pub client_code: Option<String>,
    pub diagnosis: Option<String>,
    pub therapies: Option<String>, // rreshtat Rp.; bosh => vija per plotesim me dore
}

pub fn render_prescription_pdf(data: &PrescriptionPdfData) -> anyhow::Result<Vec<u8>> {
    let (doc, page1, layer1) = PdfDocument::new("Recete", Mm(PAGE_W), Mm(PAGE_H), "Layer 1");
    let font   = doc.add_builtin_font(BuiltinFont::Helvetica).context("font")?;
    let font_b = doc.add_builtin_font(BuiltinFont::HelveticaBold).context("font bold")?;

    let layer = doc.get_page(page1).get_layer(layer1);

    let doc_name = opt(&data.doctor_name);
    let subtitle = if doc_name.is_empty() {
        data.clinic_name.clone()
    } else {
        format!("{}  |  Dr. {}", data.clinic_name, doc_name)
    };

    let mut y = render_header(
        &layer, &font, &font_b,
        "RECETE MJEKESORE", &subtitle,
        "", &format!("Data: {}", fmt_date(&data.date)),
        data.header_png.as_deref(), data.logo_png.as_deref(),
    );

    // ── Pacienti ───────────────────────────────────────────────────────────
    y -= 2.0;
    ctxt_l(&layer, &font_b, ML, y, "Pacienti", 9.5, c_navy_text());
    y -= LH + 1.0;

    let name = opt(&data.client_name).trim().to_string();
    let dob  = fmt_date(opt(&data.client_dob));
    let code = opt(&data.client_code).trim().to_string();

    // Emri (gjysma e majte) + Datelindja (gjysma e djathte) — me vija kur mungojne.
    let mid = ML + CW * 0.55;
    ctxt_l(&layer, &font, ML, y, "Emri:", 9.0, c_label());
    if name.is_empty() {
        hline(&layer, ML + 12.0, mid - 6.0, y - 1.0, 0.4, c_label());
    } else {
        txt_l(&layer, &font_b, ML + 12.0, y, &clamp_text(&name, 34), 10.0);
    }
    ctxt_l(&layer, &font, mid, y, "Datelindja:", 9.0, c_label());
    if dob.trim().is_empty() || dob.trim() == "-" {
        hline(&layer, mid + 20.0, CR, y - 1.0, 0.4, c_label());
    } else {
        txt_l(&layer, &font_b, mid + 20.0, y, &dob, 10.0);
    }
    y -= LH + 2.0;

    if !code.is_empty() {
        ctxt_l(&layer, &font, ML, y, "Kodi i pacientit:", 9.0, c_label());
        txt_l(&layer, &font, ML + 26.0, y, &code, 9.0);
        y -= LH + 1.0;
    }

    // ── Diagnoza ───────────────────────────────────────────────────────────
    ctxt_l(&layer, &font, ML, y, "Diagnoza:", 9.0, c_label());
    let diag = opt(&data.diagnosis).trim().to_string();
    if diag.is_empty() {
        hline(&layer, ML + 18.0, CR, y - 1.0, 0.4, c_label());
        y -= LH + 2.0;
        hline(&layer, ML, CR, y - 1.0, 0.4, c_label());
        y -= LH + 2.0;
    } else {
        let mut first = true;
        for line in diag.lines().take(3) {
            let x = if first { ML + 18.0 } else { ML };
            txt_l(&layer, &font, x, y, &clamp_text(line, 88), 9.0);
            y -= LH;
            first = false;
        }
        y -= 2.0;
    }

    // ── Rp./ ───────────────────────────────────────────────────────────────
    y -= 3.0;
    hline(&layer, ML, CR, y, 1.2, c_navy());
    y -= LH + 3.0;
    ctxt_l(&layer, &font_b, ML, y, "Rp./", 16.0, c_navy_text());
    y -= LH + 4.0;

    let therapies = opt(&data.therapies).trim().to_string();
    if therapies.is_empty() {
        // Recete e zbrazet: 8 vija per plotesim me dore.
        for _ in 0..8 {
            hline(&layer, ML + 4.0, CR, y - 1.0, 0.4, c_label());
            y -= LH + 4.5;
        }
    } else {
        for line in therapies.lines() {
            let t = line.trim();
            if t.is_empty() {
                y -= LH * 0.6;
                continue;
            }
            txt_l(&layer, &font, ML + 4.0, y, &clamp_text(t, 92), 10.5);
            y -= LH + 1.5;
        }
        // Disa vija shtese per plotesime.
        for _ in 0..2 {
            hline(&layer, ML + 4.0, CR, y - 1.0, 0.4, c_label());
            y -= LH + 4.5;
        }
    }

    // ── Nenshkrimi + vula (fiksuar poshte) ────────────────────────────────
    let sig_y: f32 = 42.0;
    let title_line = {
        let t = opt(&data.doctor_title).trim().to_string();
        if t.is_empty() { String::new() } else { format!("{} ", t) }
    };
    let sig_label = if doc_name.is_empty() {
        "Mjeku".to_string()
    } else {
        format!("{}Dr. {}", title_line, doc_name)
    };

    ctxt_l(&layer, &font, ML, sig_y, "Vula:", 9.0, c_label());
    hline(&layer, ML, ML + 55.0, sig_y - 16.0, 0.4, c_label());

    hline(&layer, CR - 65.0, CR, sig_y - 2.0, 0.5, c_label());
    ctxt_l(&layer, &font, CR - 65.0, sig_y - 7.0, &sig_label, 9.0, c_label());
    ctxt_l(&layer, &font, CR - 65.0, sig_y - 12.0, "Nenshkrimi i mjekut", 8.0, c_label());

    // Footer i vogel.
    ctxt_l(&layer, &font, ML, 14.0, &format!("{} — Recete e leshuar me {}", data.clinic_name, fmt_date(&data.date)), 7.5, c_label());

    save_pdf(doc)
}
