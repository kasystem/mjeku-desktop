use std::io::{BufWriter, Cursor};

use anyhow::{anyhow, Context};
use printpdf::{
  BuiltinFont, Image, ImageTransform, IndirectFontRef, Mm, PdfDocument, PdfDocumentReference, PdfLayerReference, PdfPageIndex,
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
  let font = doc.add_builtin_font(BuiltinFont::Helvetica).context("add font")?;
  let font_b = doc.add_builtin_font(BuiltinFont::HelveticaBold).context("add bold font")?;

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
    let decoder = printpdf::image_crate::codecs::png::PngDecoder::new(&mut cur).context("decode header png")?;
    let img = Image::try_from(decoder).context("load header image")?;

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
  } else {
    write_line_with_font(
      &doc,
      &mut page,
      &mut layer,
      &mut y,
      left,
      lh,
      &font_b,
      data.clinic_name.clone(),
      18.0,
    )?;
    y -= 2.0;
  }

  write_line_with_font(&doc, &mut page, &mut layer, &mut y, left, lh, &font_b, "FATURË".to_string(), 14.0)?;
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
  write_line_with_font(&doc, &mut page, &mut layer, &mut y, left, lh, &font, "".to_string(), 10.0)?;

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
  if let Some(v) = data.client_code.as_deref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
    write_line_with_font(&doc, &mut page, &mut layer, &mut y, left, lh, &font, format!("Kodi: {v}"), 10.5)?;
  }
  if let Some(v) = data.client_dob.as_deref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
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
    write_line_with_font(&doc, &mut page, &mut layer, &mut y, left, lh, &font, format!("Adresa: {v}"), 10.5)?;
  }
  if let Some(v) = data.client_phone.as_deref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
    write_line_with_font(&doc, &mut page, &mut layer, &mut y, left, lh, &font, format!("Tel: {v}"), 10.5)?;
  }
  if let Some(v) = data.client_email.as_deref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
    write_line_with_font(&doc, &mut page, &mut layer, &mut y, left, lh, &font, format!("Email: {v}"), 10.5)?;
  }
  if let Some(v) = data.notes.as_deref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
    write_line_with_font(&doc, &mut page, &mut layer, &mut y, left, lh, &font, format!("Shënime: {v}"), 10.5)?;
  }

  write_line_with_font(&doc, &mut page, &mut layer, &mut y, left, lh, &font, "".to_string(), 10.0)?;
  write_line_with_font(
    &doc,
    &mut page,
    &mut layer,
    &mut y,
    left,
    lh,
    &font_b,
    "Nr | Përshkrimi                               | Sasia | Çmimi  | TVSH | Totali".to_string(),
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
    "--------------------------------------------------------------------------------".to_string(),
    9.0,
  )?;

  let mut subtotal = 0.0_f64;
  let mut vat8 = 0.0_f64;
  let mut vat18 = 0.0_f64;

  if data.lines.is_empty() {
    write_line_with_font(&doc, &mut page, &mut layer, &mut y, left, lh, &font, "(pa rreshta)".to_string(), 10.0)?;
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
    "--------------------------------------------------------------------------------".to_string(),
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
      format!("TVSH e përfshirë në çmim: 8% = {} | 18% = {}", money(vat8), money(vat18)),
      10.0,
    )?;
  }

  write_line_with_font(&doc, &mut page, &mut layer, &mut y, left, lh, &font, "".to_string(), 10.0)?;
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

  write_line_with_font(&doc, &mut page, &mut layer, &mut y, left, lh, &font, "".to_string(), 10.0)?;
  write_line_with_font(
    &doc,
    &mut page,
    &mut layer,
    &mut y,
    left,
    lh,
    &font,
    "Dokument PDF i gjeneruar nga aplikacioni Mjeku.".to_string(),
    9.0,
  )?;

  let mut writer = BufWriter::new(Cursor::new(Vec::<u8>::new()));
  doc.save(&mut writer).map_err(|e| anyhow!("save pdf: {e}"))?;
  let cursor = writer.into_inner().map_err(|e| anyhow!("save pdf: {e}"))?;
  Ok(cursor.into_inner())
}

