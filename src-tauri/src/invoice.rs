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
  // If the price is VAT-inclusive: VAT = gross - gross/(1+rate)
  gross - (gross / (1.0 + rate))
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

  let mut y = (page_h - top) as f32;
  let lh = 6.2_f32;

  // Optional header image (PNG) set by the clinic.
  if let Some(bytes) = data.header_png.as_deref() {
    let mut cur = Cursor::new(bytes);
    let decoder = printpdf::image_crate::codecs::png::PngDecoder::new(&mut cur).context("decode header png")?;
    let img = Image::try_from(decoder).context("load header image")?;

    let w_px = img.image.width.0 as f32;
    let h_px = img.image.height.0 as f32;
    if w_px > 0.0 && h_px > 0.0 {
      let target_w_mm = page_w - left - right;
      let header_h_mm = target_w_mm * (h_px / w_px);
      let top_y = page_h - top;
      let lower_y = top_y - header_h_mm;

      // Fit to width, keep aspect ratio.
      let dpi: f32 = 300.0;
      let scale: f32 = (target_w_mm as f32) * dpi / ((w_px as f32) * 25.4);
      img.add_to_layer(
        layer.clone(),
        ImageTransform {
          translate_x: Some(Mm(left)),
          translate_y: Some(Mm(lower_y)),
          rotate: None,
          scale_x: Some(scale),
          scale_y: Some(scale),
          dpi: Some(dpi),
        },
      );

      // Start text below the header.
      y = (lower_y - 6.0) as f32;
    }
  } else {
    // Fallback without header image.
    layer.use_text(data.clinic_name.clone(), 18.0, Mm(left), Mm(y), &font_b);
    y -= lh + 2.0;
  }

  layer.use_text("Faturë (PDF)".to_string(), 12.0, Mm(left), Mm(y), &font_b);
  y -= lh;
  write_line_with_font(&doc, &mut page, &mut layer, &mut y, left, lh, &font, format!("ID: {}", data.invoice_id), 10.5)?;
  if let Some(d) = data.date.as_deref().filter(|s| !s.trim().is_empty()) {
    write_line_with_font(&doc, &mut page, &mut layer, &mut y, left, lh, &font, format!("Data: {d}"), 10.5)?;
  }
  write_line_with_font(&doc, &mut page, &mut layer, &mut y, left, lh, &font, "".to_string(), 10.0)?;

  layer.use_text(format!("Pacienti: {}", data.client_name), 12.0, Mm(left), Mm(y), &font_b);
  y -= lh;
  if data.client_code.as_deref().unwrap_or("").trim().len() > 0 {
    write_line_with_font(
      &doc,
      &mut page,
      &mut layer,
      &mut y,
      left,
      lh,
      &font,
      format!("Kodi: {}", data.client_code.as_deref().unwrap_or("")),
      10.5,
    )?;
  }
  if data.client_dob.as_deref().unwrap_or("").trim().len() > 0 {
    write_line_with_font(
      &doc,
      &mut page,
      &mut layer,
      &mut y,
      left,
      lh,
      &font,
      format!("Data e lindjes: {}", data.client_dob.as_deref().unwrap_or("")),
      10.5,
    )?;
  }
  if data.client_address.as_deref().unwrap_or("").trim().len() > 0 || data.client_city.as_deref().unwrap_or("").trim().len() > 0 {
    let addr = data.client_address.as_deref().unwrap_or("").trim();
    let city = data.client_city.as_deref().unwrap_or("").trim();
    let mut line = String::new();
    if !addr.is_empty() {
      line.push_str(addr);
    }
    if !city.is_empty() {
      if !line.is_empty() {
        line.push_str(", ");
      }
      line.push_str(city);
    }
    if !line.is_empty() {
      write_line_with_font(&doc, &mut page, &mut layer, &mut y, left, lh, &font, format!("Adresa: {}", line), 10.5)?;
    }
  }
  if data.client_phone.as_deref().unwrap_or("").trim().len() > 0 {
    write_line_with_font(
      &doc,
      &mut page,
      &mut layer,
      &mut y,
      left,
      lh,
      &font,
      format!("Tel: {}", data.client_phone.as_deref().unwrap_or("")),
      10.5,
    )?;
  }
  if data.client_email.as_deref().unwrap_or("").trim().len() > 0 {
    write_line_with_font(
      &doc,
      &mut page,
      &mut layer,
      &mut y,
      left,
      lh,
      &font,
      format!("Email: {}", data.client_email.as_deref().unwrap_or("")),
      10.5,
    )?;
  }
  if let Some(n) = data.notes.as_deref().filter(|s| !s.trim().is_empty()) {
    write_line_with_font(&doc, &mut page, &mut layer, &mut y, left, lh, &font, format!("Shenime: {}", n.trim()), 10.5)?;
  }

  write_line_with_font(&doc, &mut page, &mut layer, &mut y, left, lh, &font, "".to_string(), 10.0)?;
  layer.use_text("Procedurat".to_string(), 12.0, Mm(left), Mm(y), &font_b);
  y -= lh;
  write_line_with_font(
    &doc,
    &mut page,
    &mut layer,
    &mut y,
    left,
    lh,
    &font,
    "Dh | Procedura                       | Sasia | Cmimi | Totali | F | TVSH".to_string(),
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
    "-------------------------------------------------------------------------------".to_string(),
    9.0,
  )?;

  if data.lines.is_empty() {
    write_line_with_font(&doc, &mut page, &mut layer, &mut y, left, lh, &font, "(pa rreshta)".to_string(), 10.0)?;
  } else {
    let mut vat8 = 0.0_f64;
    let mut vat18 = 0.0_f64;
    for ln in &data.lines {
      let tooth = ln.tooth.clone().unwrap_or_else(|| "".to_string());
      let title = if ln.title.len() > 32 {
        format!("{}...", &ln.title[..29])
      } else {
        ln.title.clone()
      };
      let sub = ln.qty * ln.unit_price;
      let rate = vat_rate_for(&ln.vat_code);
      let vat = vat_included_amount(sub, rate);
      if (rate - 0.08).abs() < 0.000_000_1 {
        vat8 += vat;
      } else if (rate - 0.18).abs() < 0.000_000_1 {
        vat18 += vat;
      }
      let fiscal = if ln.fiscal { "Po" } else { "Jo" };
      let line = format!(
        "{:>2} | {:<30} | {:>4} | {:>5} | {:>6} | {} | {}",
        tooth,
        title,
        money(ln.qty),
        money(ln.unit_price),
        money(sub),
        fiscal,
        ln.vat_code.trim().to_uppercase()
      );
      write_line_with_font(&doc, &mut page, &mut layer, &mut y, left, lh, &font, line, 9.0)?;
    }

    // VAT summary (assuming prices are VAT-inclusive).
    if vat8 > 0.0 || vat18 > 0.0 {
      write_line_with_font(&doc, &mut page, &mut layer, &mut y, left, lh, &font, "".to_string(), 10.0)?;
      write_line_with_font(
        &doc,
        &mut page,
        &mut layer,
        &mut y,
        left,
        lh,
        &font,
        format!("TVSH (përfshirë në çmim): 8% = {} | 18% = {}", money(vat8), money(vat18)),
        10.0,
      )?;
    }
  }

  write_line_with_font(&doc, &mut page, &mut layer, &mut y, left, lh, &font, "".to_string(), 10.0)?;
  layer.use_text(format!("Totali: {}", money(data.total)), 12.0, Mm(left), Mm(y), &font_b);
  y -= lh;
  write_line_with_font(&doc, &mut page, &mut layer, &mut y, left, lh, &font, format!("Fiskal: {}", money(data.fiscal_total)), 10.5)?;
  write_line_with_font(
    &doc,
    &mut page,
    &mut layer,
    &mut y,
    left,
    lh,
    &font,
    format!("Jo-fiskal: {}", money(data.non_fiscal_total)),
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
    &font,
    "Shënim: Ky PDF është gjeneruar nga aplikacioni Mjeku (offline-first).".to_string(),
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
    "Kupon fiskal mund të jetë i ndarë sipas rreshtave (Fiskal: Po/Jo).".to_string(),
    9.0,
  )?;

  let mut writer = BufWriter::new(Cursor::new(Vec::<u8>::new()));
  doc.save(&mut writer).map_err(|e| anyhow!("save pdf: {e}"))?;
  let cursor = writer.into_inner().map_err(|e| anyhow!("save pdf: {e}"))?;
  Ok(cursor.into_inner())
}
