use std::io::Cursor;
use std::io::BufWriter;

use anyhow::{anyhow, Context};
use printpdf::{BuiltinFont, Mm, PdfDocument};

#[derive(Debug, Clone)]
pub struct InvoiceLine {
  pub tooth: Option<String>,
  pub title: String,
  pub qty: f64,
  pub unit_price: f64,
  pub fiscal: bool,
}

#[derive(Debug, Clone)]
pub struct InvoicePdfData {
  pub clinic_name: String,
  pub invoice_id: String,
  pub date: Option<String>,
  pub client_name: String,
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

pub fn render_invoice_pdf(data: &InvoicePdfData) -> anyhow::Result<Vec<u8>> {
  let (doc, page1, layer1) = PdfDocument::new("Fature", Mm(210.0), Mm(297.0), "Layer 1");
  let font = doc
    .add_builtin_font(BuiltinFont::Helvetica)
    .context("add font")?;

  let mut page = page1;
  let mut layer = doc.get_page(page).get_layer(layer1);

  let mut y = 286.0_f32;
  let left = 14.0_f32;
  let lh = 6.2_f32;

  let mut write_line = |text: String, size: f32| -> anyhow::Result<()> {
    if y < 18.0 {
      let (p, l) = doc.add_page(Mm(210.0), Mm(297.0), "Layer");
      page = p;
      layer = doc.get_page(page).get_layer(l);
      y = 286.0;
    }
    layer.use_text(text, size, Mm(left), Mm(y), &font);
    y -= lh;
    Ok(())
  };

  write_line(data.clinic_name.clone(), 18.0)?;
  write_line(format!("Fature (PDF)"), 12.0)?;
  write_line(format!("ID: {}", data.invoice_id), 10.5)?;
  if let Some(d) = data.date.as_deref().filter(|s| !s.trim().is_empty()) {
    write_line(format!("Data: {d}"), 10.5)?;
  }
  write_line("".to_string(), 10.0)?;

  write_line(format!("Pacienti: {}", data.client_name), 12.0)?;
  if data.client_phone.as_deref().unwrap_or("").trim().len() > 0 {
    write_line(format!("Tel: {}", data.client_phone.as_deref().unwrap_or("")), 10.5)?;
  }
  if data.client_email.as_deref().unwrap_or("").trim().len() > 0 {
    write_line(format!("Email: {}", data.client_email.as_deref().unwrap_or("")), 10.5)?;
  }
  if let Some(n) = data.notes.as_deref().filter(|s| !s.trim().is_empty()) {
    write_line(format!("Shenime: {}", n.trim()), 10.5)?;
  }

  write_line("".to_string(), 10.0)?;
  write_line("Procedurat:".to_string(), 12.0)?;
  write_line("Dh | Procedura                         | Sasia | Cmimi | Totali | Fiskal".to_string(), 9.2)?;
  write_line("--------------------------------------------------------------------------------".to_string(), 9.2)?;

  if data.lines.is_empty() {
    write_line("(pa rreshta)".to_string(), 10.0)?;
  } else {
    for ln in &data.lines {
      let tooth = ln.tooth.clone().unwrap_or_else(|| "".to_string());
      let title = if ln.title.len() > 32 {
        format!("{}...", &ln.title[..29])
      } else {
        ln.title.clone()
      };
      let sub = ln.qty * ln.unit_price;
      let fiscal = if ln.fiscal { "Po" } else { "Jo" };
      let line = format!(
        "{:>2} | {:<32} | {:>4} | {:>5} | {:>6} | {}",
        tooth,
        title,
        money(ln.qty),
        money(ln.unit_price),
        money(sub),
        fiscal
      );
      write_line(line, 9.2)?;
    }
  }

  write_line("".to_string(), 10.0)?;
  write_line(format!("Totali: {}", money(data.total)), 12.0)?;
  write_line(format!("Fiskal: {}", money(data.fiscal_total)), 10.5)?;
  write_line(format!("Jo-fiskal: {}", money(data.non_fiscal_total)), 10.5)?;

  write_line("".to_string(), 10.0)?;
  write_line("Shenim: Ky PDF eshte gjeneruar nga aplikacioni Mjeku (offline-first).".to_string(), 9.2)?;
  write_line("Kupon fiskal mund te jete i ndare sipas rreshtave (Fiskal: Po/Jo).".to_string(), 9.2)?;

  let mut writer = BufWriter::new(Cursor::new(Vec::<u8>::new()));
  doc.save(&mut writer).map_err(|e| anyhow!("save pdf: {e}"))?;
  let cursor = writer.into_inner().map_err(|e| anyhow!("save pdf: {e}"))?;
  Ok(cursor.into_inner())
}
