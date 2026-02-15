use std::io::{BufWriter, Cursor};

use anyhow::{anyhow, Context};
use printpdf::{
    BuiltinFont, Image, ImageTransform, IndirectFontRef, Mm, PdfDocument, PdfDocumentReference,
    PdfLayerReference, PdfPageIndex,
};

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

fn money(n: f64) -> String {
    if !n.is_finite() {
        return "0.00".to_string();
    }
    format!("{:.2}", n)
}

fn vat_rate_for(code: &str) -> f64 {
    match code.trim().to_uppercase().as_str() {
        "D" => 0.08,
        "E" => 0.18,
        _ => 0.0,
    }
}

fn vat_included_amount(gross: f64, rate: f64) -> f64 {
    if rate <= 0.0 {
        return 0.0;
    }
    gross - (gross / (1.0 + rate))
}

fn clamp_text(s: &str, max: usize) -> String {
    let t = s.trim();
    if t.chars().count() <= max {
        return t.to_string();
    }
    let mut out = String::new();
    for ch in t.chars().take(max.saturating_sub(3)) {
        out.push(ch);
    }
    out.push_str("...");
    out
}

fn estimate_text_width_mm(text: &str, font_size: f32) -> f32 {
    // Rough width estimate for Helvetica in mm (good enough for footer centering).
    text.chars().count() as f32 * font_size * 0.17
}

fn draw_footer_centered(
    layer: &PdfLayerReference,
    font: &IndirectFontRef,
    page_w: f32,
    y: f32,
    text: &str,
    size: f32,
) {
    let w = estimate_text_width_mm(text, size);
    let mut x = (page_w - w) / 2.0;
    if x < 10.0 {
        x = 10.0;
    }
    layer.use_text(text.to_string(), size, Mm(x), Mm(y), font);
}

fn write_line_with_font(
    doc: &PdfDocumentReference,
    page: &mut PdfPageIndex,
    layer: &mut PdfLayerReference,
    y: &mut f32,
    left: f32,
    lh: f32,
    font: &IndirectFontRef,
    text: String,
    size: f32,
) -> anyhow::Result<()> {
    if *y < 18.0 {
        let (p, l) = doc.add_page(Mm(210.0), Mm(297.0), "Layer");
        *page = p;
        *layer = doc.get_page(*page).get_layer(l);
        *y = 286.0;
    }
    layer.use_text(text, size, Mm(left), Mm(*y), font);
    *y -= lh;
    Ok(())
}

pub fn render_invoice_pdf(data: &InvoicePdfData) -> anyhow::Result<Vec<u8>> {
    let (doc, page1, layer1) = PdfDocument::new("Fature", Mm(210.0), Mm(297.0), "Layer 1");
    let font = doc
        .add_builtin_font(BuiltinFont::Helvetica)
        .context("add font")?;
    let font_b = doc
        .add_builtin_font(BuiltinFont::HelveticaBold)
        .context("add bold font")?;

    let mut page = page1;
    let mut layer = doc.get_page(page).get_layer(layer1);

    let page_w = 210.0_f32;
    let page_h = 297.0_f32;
    let left = 14.0_f32;
    let right = 14.0_f32;
    let top = 12.0_f32;
    let mut y = page_h - top;
    let lh = 6.2_f32;

    if let Some(bytes) = data.header_png.as_deref() {
        let mut cur = Cursor::new(bytes);
        if let Ok(decoder) = printpdf::image_crate::codecs::png::PngDecoder::new(&mut cur) {
            if let Ok(img) = Image::try_from(decoder) {
                let w_px = img.image.width.0 as f32;
                let h_px = img.image.height.0 as f32;
                if w_px > 0.0 && h_px > 0.0 {
                    let available_w = page_w - left - right;
                    let natural_h = available_w * (h_px / w_px);
                    let max_h = 85.0_f32;
                    let (draw_w, draw_h) = if natural_h <= max_h {
                        (available_w, natural_h)
                    } else {
                        (max_h * (w_px / h_px), max_h)
                    };
                    let x = left + ((available_w - draw_w) / 2.0);
                    let lower_y = page_h - top - draw_h;

                    let dpi: f32 = 300.0;
                    let scale_x: f32 = draw_w * dpi / (w_px * 25.4);
                    let scale_y: f32 = draw_h * dpi / (h_px * 25.4);
                    img.add_to_layer(
                        layer.clone(),
                        ImageTransform {
                            translate_x: Some(Mm(x)),
                            translate_y: Some(Mm(lower_y)),
                            rotate: None,
                            scale_x: Some(scale_x),
                            scale_y: Some(scale_y),
                            dpi: Some(dpi),
                        },
                    );

                    y = lower_y - 8.0;
                }
            }
        }
    }

    if let Some(bytes) = data.logo_png.as_deref() {
        let mut cur = Cursor::new(bytes);
        if let Ok(decoder) = printpdf::image_crate::codecs::png::PngDecoder::new(&mut cur) {
            if let Ok(img) = Image::try_from(decoder) {
                let w_px = img.image.width.0 as f32;
                let h_px = img.image.height.0 as f32;
                if w_px > 0.0 && h_px > 0.0 {
                    let max_w = 28.0_f32;
                    let max_h = 18.0_f32;
                    let mut draw_w = max_w;
                    let mut draw_h = max_w * (h_px / w_px);
                    if draw_h > max_h {
                        draw_h = max_h;
                        draw_w = max_h * (w_px / h_px);
                    }
                    let x = page_w - right - draw_w;
                    let lower_y = page_h - top - draw_h;
                    let dpi: f32 = 300.0;
                    let scale_x: f32 = draw_w * dpi / (w_px * 25.4);
                    let scale_y: f32 = draw_h * dpi / (h_px * 25.4);
                    img.add_to_layer(
                        layer.clone(),
                        ImageTransform {
                            translate_x: Some(Mm(x)),
                            translate_y: Some(Mm(lower_y)),
                            rotate: None,
                            scale_x: Some(scale_x),
                            scale_y: Some(scale_y),
                            dpi: Some(dpi),
                        },
                    );
                    if y > (lower_y - 4.0) {
                        y = lower_y - 4.0;
                    }
                }
            }
        }
    }

    write_line_with_font(
        &doc,
        &mut page,
        &mut layer,
        &mut y,
        left,
        lh,
        &font_b,
        "FATURË".to_string(),
        14.0,
    )?;
    let date_str = data
        .date
        .as_deref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "-".to_string());
    write_line_with_font(
        &doc,
        &mut page,
        &mut layer,
        &mut y,
        left,
        lh,
        &font,
        format!("Nr: {}    Data: {}", data.invoice_id, date_str),
        10.5,
    )?;
    write_line_with_font(
        &doc,
        &mut page,
        &mut layer,
        &mut y,
        left,
        lh,
        &font,
        "".to_string(),
        10.0,
    )?;

    write_line_with_font(
        &doc,
        &mut page,
        &mut layer,
        &mut y,
        left,
        lh,
        &font_b,
        "Të dhënat e pacientit".to_string(),
        11.5,
    )?;
    write_line_with_font(
        &doc,
        &mut page,
        &mut layer,
        &mut y,
        left,
        lh,
        &font,
        format!("Emri: {}", data.client_name),
        10.5,
    )?;
    if let Some(v) = data
        .client_code
        .as_deref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        write_line_with_font(
            &doc,
            &mut page,
            &mut layer,
            &mut y,
            left,
            lh,
            &font,
            format!("Kodi: {v}"),
            10.5,
        )?;
    }
    if let Some(v) = data
        .client_dob
        .as_deref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        write_line_with_font(
            &doc,
            &mut page,
            &mut layer,
            &mut y,
            left,
            lh,
            &font,
            format!("Data e lindjes: {v}"),
            10.5,
        )?;
    }
    let addr = data.client_address.as_deref().unwrap_or("").trim();
    let city = data.client_city.as_deref().unwrap_or("").trim();
    if !addr.is_empty() || !city.is_empty() {
        let mut v = String::new();
        if !addr.is_empty() {
            v.push_str(addr);
        }
        if !city.is_empty() {
            if !v.is_empty() {
                v.push_str(", ");
            }
            v.push_str(city);
        }
        write_line_with_font(
            &doc,
            &mut page,
            &mut layer,
            &mut y,
            left,
            lh,
            &font,
            format!("Adresa: {v}"),
            10.5,
        )?;
    }
    if let Some(v) = data
        .client_phone
        .as_deref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        write_line_with_font(
            &doc,
            &mut page,
            &mut layer,
            &mut y,
            left,
            lh,
            &font,
            format!("Tel: {v}"),
            10.5,
        )?;
    }
    if let Some(v) = data
        .client_email
        .as_deref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        write_line_with_font(
            &doc,
            &mut page,
            &mut layer,
            &mut y,
            left,
            lh,
            &font,
            format!("Email: {v}"),
            10.5,
        )?;
    }
    if let Some(v) = data
        .notes
        .as_deref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        write_line_with_font(
            &doc,
            &mut page,
            &mut layer,
            &mut y,
            left,
            lh,
            &font,
            format!("Shënime: {v}"),
            10.5,
        )?;
    }

    write_line_with_font(
        &doc,
        &mut page,
        &mut layer,
        &mut y,
        left,
        lh,
        &font,
        "".to_string(),
        10.0,
    )?;
    write_line_with_font(
        &doc,
        &mut page,
        &mut layer,
        &mut y,
        left,
        lh,
        &font_b,
        "Nr | Përshkrimi                               | Sasia | Çmimi  | TVSH | Totali"
            .to_string(),
        9.2,
    )?;
    write_line_with_font(
        &doc,
        &mut page,
        &mut layer,
        &mut y,
        left,
        lh,
        &font,
        "--------------------------------------------------------------------------------"
            .to_string(),
        9.0,
    )?;

    let mut subtotal = 0.0_f64;
    let mut vat8 = 0.0_f64;
    let mut vat18 = 0.0_f64;

    if data.lines.is_empty() {
        write_line_with_font(
            &doc,
            &mut page,
            &mut layer,
            &mut y,
            left,
            lh,
            &font,
            "(pa rreshta)".to_string(),
            10.0,
        )?;
    } else {
        for (idx, ln) in data.lines.iter().enumerate() {
            let tooth = ln.tooth.as_deref().unwrap_or("").trim();
            let description = if tooth.is_empty() {
                ln.title.clone()
            } else {
                format!("Dh {} - {}", tooth, ln.title)
            };
            let description = clamp_text(&description, 38);
            let vat_code = ln.vat_code.trim().to_uppercase();
            let sub = ln.qty * ln.unit_price;
            subtotal += sub;

            let rate = vat_rate_for(&vat_code);
            let vat = vat_included_amount(sub, rate);
            if (rate - 0.08).abs() < 0.000_000_1 {
                vat8 += vat;
            } else if (rate - 0.18).abs() < 0.000_000_1 {
                vat18 += vat;
            }

            write_line_with_font(
                &doc,
                &mut page,
                &mut layer,
                &mut y,
                left,
                lh,
                &font,
                format!(
                    "{:>2} | {:<38} | {:>5} | {:>6} | {:>4} | {:>7}",
                    idx + 1,
                    description,
                    money(ln.qty),
                    money(ln.unit_price),
                    vat_code,
                    money(sub)
                ),
                9.0,
            )?;
        }
    }

    write_line_with_font(
        &doc,
        &mut page,
        &mut layer,
        &mut y,
        left,
        lh,
        &font,
        "--------------------------------------------------------------------------------"
            .to_string(),
        9.0,
    )?;
    write_line_with_font(
        &doc,
        &mut page,
        &mut layer,
        &mut y,
        left,
        lh,
        &font,
        format!("Nëntotali: {}", money(subtotal)),
        10.5,
    )?;
    if vat8 > 0.0 || vat18 > 0.0 {
        write_line_with_font(
            &doc,
            &mut page,
            &mut layer,
            &mut y,
            left,
            lh,
            &font,
            format!(
                "TVSH e përfshirë në çmim: 8% = {} | 18% = {}",
                money(vat8),
                money(vat18)
            ),
            10.0,
        )?;
    }

    write_line_with_font(
        &doc,
        &mut page,
        &mut layer,
        &mut y,
        left,
        lh,
        &font,
        "".to_string(),
        10.0,
    )?;
    write_line_with_font(
        &doc,
        &mut page,
        &mut layer,
        &mut y,
        left,
        lh,
        &font_b,
        format!("Totali për pagesë: {}", money(data.total)),
        12.0,
    )?;

    if data.fiscal_total > 0.0 && data.non_fiscal_total > 0.0 {
        write_line_with_font(
            &doc,
            &mut page,
            &mut layer,
            &mut y,
            left,
            lh,
            &font,
            format!(
                "Ndarje informative: fiskal {} | jo-fiskal {}",
                money(data.fiscal_total),
                money(data.non_fiscal_total)
            ),
            9.8,
        )?;
    }

    let footer = "Dokument PDF i gjeneruar nga aplikacioni Mjeku.";
    draw_footer_centered(&layer, &font, page_w, 8.5, footer, 9.0);

    let mut writer = BufWriter::new(Cursor::new(Vec::<u8>::new()));
    doc.save(&mut writer)
        .map_err(|e| anyhow!("save pdf: {e}"))?;
    let cursor = writer.into_inner().map_err(|e| anyhow!("save pdf: {e}"))?;
    Ok(cursor.into_inner())
}

pub fn render_visit_pdf(data: &VisitPdfData) -> anyhow::Result<Vec<u8>> {
    let (doc, page1, layer1) = PdfDocument::new("Vizite", Mm(210.0), Mm(297.0), "Layer 1");
    let font = doc
        .add_builtin_font(BuiltinFont::Helvetica)
        .context("add font")?;
    let font_b = doc
        .add_builtin_font(BuiltinFont::HelveticaBold)
        .context("add bold font")?;

    let mut page = page1;
    let mut layer = doc.get_page(page).get_layer(layer1);

    let page_w = 210.0_f32;
    let page_h = 297.0_f32;
    let left = 14.0_f32;
    let right = 14.0_f32;
    let top = 12.0_f32;
    let mut y = page_h - top;
    let lh = 6.2_f32;

    if let Some(bytes) = data.header_png.as_deref() {
        let mut cur = Cursor::new(bytes);
        if let Ok(decoder) = printpdf::image_crate::codecs::png::PngDecoder::new(&mut cur) {
            if let Ok(img) = Image::try_from(decoder) {
                let w_px = img.image.width.0 as f32;
                let h_px = img.image.height.0 as f32;
                if w_px > 0.0 && h_px > 0.0 {
                    let available_w = page_w - left - right;
                    let natural_h = available_w * (h_px / w_px);
                    let max_h = 85.0_f32;
                    let (draw_w, draw_h) = if natural_h <= max_h {
                        (available_w, natural_h)
                    } else {
                        (max_h * (w_px / h_px), max_h)
                    };
                    let x = left + ((available_w - draw_w) / 2.0);
                    let lower_y = page_h - top - draw_h;

                    let dpi: f32 = 300.0;
                    let scale_x: f32 = draw_w * dpi / (w_px * 25.4);
                    let scale_y: f32 = draw_h * dpi / (h_px * 25.4);
                    img.add_to_layer(
                        layer.clone(),
                        ImageTransform {
                            translate_x: Some(Mm(x)),
                            translate_y: Some(Mm(lower_y)),
                            rotate: None,
                            scale_x: Some(scale_x),
                            scale_y: Some(scale_y),
                            dpi: Some(dpi),
                        },
                    );
                    y = lower_y - 8.0;
                }
            }
        }
    }

    if let Some(bytes) = data.logo_png.as_deref() {
        let mut cur = Cursor::new(bytes);
        if let Ok(decoder) = printpdf::image_crate::codecs::png::PngDecoder::new(&mut cur) {
            if let Ok(img) = Image::try_from(decoder) {
                let w_px = img.image.width.0 as f32;
                let h_px = img.image.height.0 as f32;
                if w_px > 0.0 && h_px > 0.0 {
                    let max_w = 28.0_f32;
                    let max_h = 18.0_f32;
                    let mut draw_w = max_w;
                    let mut draw_h = max_w * (h_px / w_px);
                    if draw_h > max_h {
                        draw_h = max_h;
                        draw_w = max_h * (w_px / h_px);
                    }
                    let x = page_w - right - draw_w;
                    let lower_y = page_h - top - draw_h;
                    let dpi: f32 = 300.0;
                    let scale_x: f32 = draw_w * dpi / (w_px * 25.4);
                    let scale_y: f32 = draw_h * dpi / (h_px * 25.4);
                    img.add_to_layer(
                        layer.clone(),
                        ImageTransform {
                            translate_x: Some(Mm(x)),
                            translate_y: Some(Mm(lower_y)),
                            rotate: None,
                            scale_x: Some(scale_x),
                            scale_y: Some(scale_y),
                            dpi: Some(dpi),
                        },
                    );
                    if y > (lower_y - 4.0) {
                        y = lower_y - 4.0;
                    }
                }
            }
        }
    }

    write_line_with_font(
        &doc,
        &mut page,
        &mut layer,
        &mut y,
        left,
        lh,
        &font_b,
        "RAPORTI I VIZITËS".to_string(),
        14.0,
    )?;
    let date_str = data
        .date
        .as_deref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "-".to_string());
    let time_str = data
        .visit_time
        .as_deref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "-".to_string());
    write_line_with_font(
        &doc,
        &mut page,
        &mut layer,
        &mut y,
        left,
        lh,
        &font,
        format!(
            "Nr: {}    Data: {} {}    Status: {}",
            data.visit_id, date_str, time_str, data.status
        ),
        10.5,
    )?;
    write_line_with_font(
        &doc,
        &mut page,
        &mut layer,
        &mut y,
        left,
        lh,
        &font,
        format!("Klinika: {}", data.clinic_name),
        10.5,
    )?;
    if let Some(v) = data
        .doctor_name
        .as_deref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        write_line_with_font(
            &doc,
            &mut page,
            &mut layer,
            &mut y,
            left,
            lh,
            &font,
            format!("Mjeku: {v}"),
            10.5,
        )?;
    }
    write_line_with_font(
        &doc,
        &mut page,
        &mut layer,
        &mut y,
        left,
        lh,
        &font,
        "".to_string(),
        10.0,
    )?;

    write_line_with_font(
        &doc,
        &mut page,
        &mut layer,
        &mut y,
        left,
        lh,
        &font_b,
        "Të dhënat e pacientit".to_string(),
        11.5,
    )?;
    write_line_with_font(
        &doc,
        &mut page,
        &mut layer,
        &mut y,
        left,
        lh,
        &font,
        format!("Emri: {}", data.client_name),
        10.5,
    )?;
    if let Some(v) = data
        .client_code
        .as_deref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        write_line_with_font(
            &doc,
            &mut page,
            &mut layer,
            &mut y,
            left,
            lh,
            &font,
            format!("Kodi: {v}"),
            10.5,
        )?;
    }
    if let Some(v) = data
        .client_dob
        .as_deref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        write_line_with_font(
            &doc,
            &mut page,
            &mut layer,
            &mut y,
            left,
            lh,
            &font,
            format!("Data e lindjes: {v}"),
            10.5,
        )?;
    }
    let addr = data.client_address.as_deref().unwrap_or("").trim();
    let city = data.client_city.as_deref().unwrap_or("").trim();
    if !addr.is_empty() || !city.is_empty() {
        let mut v = String::new();
        if !addr.is_empty() {
            v.push_str(addr);
        }
        if !city.is_empty() {
            if !v.is_empty() {
                v.push_str(", ");
            }
            v.push_str(city);
        }
        write_line_with_font(
            &doc,
            &mut page,
            &mut layer,
            &mut y,
            left,
            lh,
            &font,
            format!("Adresa: {v}"),
            10.5,
        )?;
    }
    if let Some(v) = data
        .client_phone
        .as_deref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        write_line_with_font(
            &doc,
            &mut page,
            &mut layer,
            &mut y,
            left,
            lh,
            &font,
            format!("Tel: {v}"),
            10.5,
        )?;
    }
    if let Some(v) = data
        .client_email
        .as_deref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        write_line_with_font(
            &doc,
            &mut page,
            &mut layer,
            &mut y,
            left,
            lh,
            &font,
            format!("Email: {v}"),
            10.5,
        )?;
    }

    write_line_with_font(
        &doc,
        &mut page,
        &mut layer,
        &mut y,
        left,
        lh,
        &font,
        "".to_string(),
        10.0,
    )?;
    write_line_with_font(
        &doc,
        &mut page,
        &mut layer,
        &mut y,
        left,
        lh,
        &font_b,
        "Parametrat e vizitës".to_string(),
        11.5,
    )?;

    let mut metric_line =
        |label: &str, value: Option<&str>, unit: Option<&str>| -> anyhow::Result<()> {
            let v = value
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty());
            if let Some(v) = v {
                let u = unit.map(|x| x.trim().to_string()).filter(|x| !x.is_empty());
                let line = match u {
                    Some(u) => format!("{label}: {v} {u}"),
                    None => format!("{label}: {v}"),
                };
                write_line_with_font(
                    &doc, &mut page, &mut layer, &mut y, left, lh, &font, line, 10.2,
                )?;
            }
            Ok(())
        };
    metric_line(
        "Pesha",
        data.body_weight.as_deref(),
        data.body_weight_unit.as_deref(),
    )?;
    metric_line(
        "Gjatesia",
        data.body_height.as_deref(),
        data.body_height_unit.as_deref(),
    )?;
    metric_line(
        "Perimetri i kokës",
        data.head_circumference.as_deref(),
        data.head_circumference_unit.as_deref(),
    )?;
    metric_line(
        "Temperatura",
        data.body_temperature.as_deref(),
        data.body_temperature_unit.as_deref(),
    )?;
    metric_line(
        "Oksigjeni në gjak",
        data.blood_oxygen.as_deref(),
        data.blood_oxygen_unit.as_deref(),
    )?;
    metric_line(
        "Glicemia",
        data.glycemia.as_deref(),
        data.glycemia_unit.as_deref(),
    )?;
    metric_line("Pulsi", data.pulse.as_deref(), data.pulse_unit.as_deref())?;
    metric_line("BMI", data.bmi.as_deref(), None)?;
    let bp_s = data
        .blood_pressure_systolic
        .as_deref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let bp_d = data
        .blood_pressure_diastolic
        .as_deref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if bp_s.is_some() || bp_d.is_some() {
        let mut bp = format!(
            "{}/{}",
            bp_s.unwrap_or_else(|| "-".to_string()),
            bp_d.unwrap_or_else(|| "-".to_string())
        );
        if let Some(u) = data
            .blood_pressure_unit
            .as_deref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
        {
            bp.push(' ');
            bp.push_str(&u);
        }
        write_line_with_font(
            &doc,
            &mut page,
            &mut layer,
            &mut y,
            left,
            lh,
            &font,
            format!("Tensioni arterial: {bp}"),
            10.2,
        )?;
    }

    let mut section_line = |label: &str, value: Option<&str>| -> anyhow::Result<()> {
        if let Some(v) = value
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
        {
            write_line_with_font(
                &doc,
                &mut page,
                &mut layer,
                &mut y,
                left,
                lh,
                &font_b,
                format!("{label}:"),
                10.5,
            )?;
            for part in v.lines() {
                let txt = part.trim();
                if txt.is_empty() {
                    continue;
                }
                write_line_with_font(
                    &doc,
                    &mut page,
                    &mut layer,
                    &mut y,
                    left + 2.0,
                    lh,
                    &font,
                    txt.to_string(),
                    10.2,
                )?;
            }
        }
        Ok(())
    };

    section_line("Ankesat", data.complaints.as_deref())?;
    section_line("Shënime shtesë", data.additional_notes.as_deref())?;
    section_line("Kontrollat", data.controls.as_deref())?;
    section_line("Vërejtjet", data.remarks.as_deref())?;
    section_line("Analizat dhe ekzaminimet", data.analyses.as_deref())?;
    section_line("Këshillat", data.advice.as_deref())?;
    section_line("Terapitë", data.therapies.as_deref())?;
    section_line("Diagnozat", data.diagnosis.as_deref())?;
    section_line("Ekzaminimet", data.examinations.as_deref())?;
    section_line("Shënime", data.notes.as_deref())?;

    if !data.lines.is_empty() {
        write_line_with_font(
            &doc,
            &mut page,
            &mut layer,
            &mut y,
            left,
            lh,
            &font,
            "".to_string(),
            10.0,
        )?;
        write_line_with_font(
            &doc,
            &mut page,
            &mut layer,
            &mut y,
            left,
            lh,
            &font_b,
            "Procedurat e vizitës".to_string(),
            11.5,
        )?;
        write_line_with_font(
            &doc,
            &mut page,
            &mut layer,
            &mut y,
            left,
            lh,
            &font_b,
            "Nr | Përshkrimi                               | Sasia | Çmimi  | Fiskal | Totali"
                .to_string(),
            9.2,
        )?;
        write_line_with_font(
            &doc,
            &mut page,
            &mut layer,
            &mut y,
            left,
            lh,
            &font,
            "--------------------------------------------------------------------------------"
                .to_string(),
            9.0,
        )?;

        for (idx, ln) in data.lines.iter().enumerate() {
            let tooth = ln.tooth.as_deref().unwrap_or("").trim();
            let description = if tooth.is_empty() {
                ln.title.clone()
            } else {
                format!("Dh {} - {}", tooth, ln.title)
            };
            let description = clamp_text(&description, 38);
            let sub = ln.qty * ln.unit_price;
            write_line_with_font(
                &doc,
                &mut page,
                &mut layer,
                &mut y,
                left,
                lh,
                &font,
                format!(
                    "{:>2} | {:<38} | {:>5} | {:>6} | {:>6} | {:>7}",
                    idx + 1,
                    description,
                    money(ln.qty),
                    money(ln.unit_price),
                    if ln.fiscal { "Po" } else { "Jo" },
                    money(sub)
                ),
                9.0,
            )?;
        }
        write_line_with_font(
            &doc,
            &mut page,
            &mut layer,
            &mut y,
            left,
            lh,
            &font,
            "--------------------------------------------------------------------------------"
                .to_string(),
            9.0,
        )?;
        write_line_with_font(
            &doc,
            &mut page,
            &mut layer,
            &mut y,
            left,
            lh,
            &font_b,
            format!("Totali i procedurave: {}", money(data.total)),
            11.0,
        )?;
    }

    let footer = "Dokument PDF i gjeneruar nga aplikacioni Mjeku.";
    draw_footer_centered(&layer, &font, page_w, 8.5, footer, 9.0);

    let mut writer = BufWriter::new(Cursor::new(Vec::<u8>::new()));
    doc.save(&mut writer)
        .map_err(|e| anyhow!("save pdf: {e}"))?;
    let cursor = writer.into_inner().map_err(|e| anyhow!("save pdf: {e}"))?;
    Ok(cursor.into_inner())
}
