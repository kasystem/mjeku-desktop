//! A4 physiotherapy care document from authoritative SQLite records, never WebView.
use anyhow::{Context,Result};
use printpdf::{BuiltinFont, Color, Image, ImageTransform, IndirectFontRef, Mm, PdfDocument,
    PdfDocumentReference, PdfLayerReference, Rgb};
use serde_json::Value;
use std::io::{BufWriter, Cursor};

const W:f32=210.0;
const H:f32=297.0;
const POS_LOGO:&[u8]=include_bytes!("../resources/programeri-pos-logo.png");
fn f<'a>(r:&'a Value,key:&str)->&'a str {r[key].as_str().unwrap_or("")}

struct Canvas {
    document:PdfDocumentReference,
    page:PdfLayerReference,
    normal:IndirectFontRef,
    strong:IndirectFontRef,
    y:f32,
    count:usize,
}
impl Canvas {
    fn new()->Result<Self>{
        let (document,p,l)=PdfDocument::new(
            "PROGRAMERI POS - Fizioterapi", Mm(W), Mm(H), "Dokument");
        let normal=document.add_builtin_font(BuiltinFont::Helvetica)?;
        let strong=document.add_builtin_font(BuiltinFont::HelveticaBold)?;
        Ok(Self{page:document.get_page(p).get_layer(l),
            document,normal,strong,y:267.0,count:1})
    }
    fn next(&mut self){
        let (p,l)=self.document.add_page(Mm(W),Mm(H),"Dokument");
        self.page=self.document.get_page(p).get_layer(l);
        self.y=267.0;self.count+=1;
        self.header();
    }
    fn header(&self){
        let mut cursor=Cursor::new(POS_LOGO);
        if let Ok(decoder)=printpdf::image_crate::codecs::png::PngDecoder::new(&mut cursor) {
            if let Ok(img)=Image::try_from(decoder) {
                let w=img.image.width.0 as f32;
                let h=img.image.height.0 as f32;
                if w>0.0 && h>0.0 {
                    let dpi=300.0_f32;
                    img.add_to_layer(self.page.clone(),ImageTransform{
                        translate_x:Some(Mm(10.0)),translate_y:Some(Mm(254.0)),
                        rotate:None,
                        scale_x:Some(35.0*dpi/(w*25.4)),
                        scale_y:Some(35.0*dpi/(h*25.4)),
                        dpi:Some(dpi)
                    });
                }
            }
        }
        self.page.set_fill_color(Color::Rgb(Rgb::new(0.0,0.42,0.51,None)));
        self.page.use_text("PROGRAMERI POS",13.0,Mm(46.0),Mm(283.0),&self.strong);
        self.page.use_text(
            format!("Fizioterapi dhe Rehabilitim  |  Faqe {}",self.count),
            9.0,Mm(46.0),Mm(276.0),&self.normal);
    }
    fn line(&mut self,s:&str,bold:bool,size:f32) {
        if self.y<22.0{self.next();}
        self.page.set_fill_color(Color::Rgb(Rgb::new(0.12,0.17,0.23,None)));
        let font=if bold {&self.strong}else{&self.normal};
        self.page.use_text(s,size,Mm(16.0),Mm(self.y),font);
        self.y-=if size>11.0{8.0}else{5.8};
    }
    fn text(&mut self,s:&str){
        // Break long narrative without depending on the WebView or print CSS.
        for para in s.replace('\r',"").split('\n'){
            if para.trim().is_empty(){self.y-=3.0;continue;}
            let mut row=String::new();
            for token in para.split_whitespace(){
                for chunk in token.chars().collect::<Vec<_>>().chunks(90){
                    let piece:String=chunk.iter().collect();
                    if row.chars().count()+piece.chars().count()+1>90 && !row.is_empty(){
                        self.line(&row,false,9.0);row.clear();
                    }
                    if !row.is_empty(){row.push(' ');}
                    row.push_str(&piece);
                }
            }
            if !row.is_empty(){self.line(&row,false,9.0);}
        }
    }
    fn section(&mut self,heading:&str){
        self.y-=4.0;
        self.line(heading,true,11.0);
    }
    fn pair(&mut self,title:&str,value:&str) {
        self.text(&format!("{}: {}",title,if value.trim().is_empty(){"—"}else{value}));
    }
    fn finish(self)->Result<Vec<u8>>{
        let mut out=BufWriter::new(Cursor::new(Vec::new()));
        self.document.save(&mut out).context("write physiotherapy PDF")?;
        let file=out.into_inner().map_err(|e|anyhow::anyhow!("PDF buffer: {e}"))?;
        Ok(file.into_inner())
    }
}

pub fn render(document:&Value,clinic:&str,kind:&str)->Result<Vec<u8>>{
    let mut pdf=Canvas::new()?;
    pdf.header();
    pdf.line(if kind=="referral"{
        "REFERIM PER FIZIOTERAPI"
    }else{
        "RAPORT KLINIK FIZIOTERAPEUTIK"
    },true,14.0);
    pdf.pair("Institucioni",clinic);
    pdf.pair("Pacienti",f(document,"patient_name"));
    pdf.pair("Diagnoza",f(document,"diagnosis"));
    pdf.pair("Regjioni",f(document,"region"));
    pdf.pair("Ana anatomike",f(document,"laterality"));
    pdf.pair("Protokolli",f(document,"protocol"));
    pdf.pair("Faza",f(document,"phase"));
    pdf.section("KUFIZIMET KLINIKE QE DUHET RESPEKTUAR");
    pdf.pair("Kufizime te ngarkeses",f(document,"load_restrictions"));
    pdf.pair("Kufizime te levizjes ROM",f(document,"rom_restrictions"));
    pdf.section("IDENTIFIKUESIT DHE AUTORI");
    pdf.pair("ID dokumenti",f(document,"id"));
    pdf.pair("Autori (ID)",f(document,"author_id"));
    pdf.pair("Data",f(document,"created_at"));
    if kind!="referral"{
        pdf.pair("Versioni",&document["version"].as_i64().unwrap_or(1).to_string());
        pdf.pair("Statusi",f(document,"status"));
        if f(document,"status")=="draft"{pdf.line("DRAFT - I PAFINALIZUAR",true,10.0);}
        if f(document,"status")=="amendment" {
            pdf.pair("Arsyeja e amendamentit",f(document,"reason"));
        }
        pdf.section("PERSHKRIMI DHE PERMBLEDHJA KLINIKE");
        pdf.text(f(document,"narrative"));
        if let Ok(snapshot)=serde_json::from_str::<Value>(f(document,"clinical_summary_json")){
            for (key,title) in [
                ("sessions","SEANCAT"),
                ("measurements","MATJET FUNKSIONALE"),
                ("care_plans","PLANI I TRAJTIMIT"),
                ("home_exercises","PROGRAMI I USHTRIMEVE")
            ] {
                let rows=snapshot[key].as_array();
                if let Some(items)=rows{
                    if items.is_empty(){continue;}
                    pdf.section(title);
                    for (i,item) in items.iter().enumerate() {
                        if i>=120{pdf.text("Lista e tejgjate: konsultoni kartelen klinike.");break;}
                        let s=match key {
                            "sessions"=>format!("{} | {} | Dhimbja {} -> {} | {}",
                                f(item,"session_date"),f(item,"intervention"),
                                item["pain_before"],item["pain_after"],f(item,"response")),
                            "measurements"=>format!("{} | {} {} | {} {} | {}",
                                f(item,"assessed_at"),f(item,"region"),f(item,"movement"),
                                item["value"],f(item,"unit"),f(item,"method")),
                            "care_plans"=>format!("v{} | {} | {}",
                                item["version"],f(item,"goal"),f(item,"measurable_target")),
                            _=>format!("{} | {} x {} | {}",
                                f(item,"title"),item["sets"],item["repetitions"],f(item,"frequency"))
                        };
                        pdf.text(&format!("{}. {}",i+1,s));
                    }
                }
            }
        }
    }else{
        pdf.section("UDHEZIMI");
        pdf.text("Vleresimi funksional dhe trajtimi dokumentohen nga profesionisti klinik. Programi dhe ushtrimet duhet te respektojne kufizimet e percaktuara me siper. Ndryshimet e fazes kerkojne miratim profesional.");
    }
    pdf.y-=8.0;
    pdf.section("GJURMUESHMERIA");
    pdf.text("Dokumenti eshte gjeneruar nga regjistrimet klinike te ruajtura. Ky PDF nuk eshte nenshkrim elektronik i kualifikuar dhe nuk zevendeson vleresimin e profesionistit.");
    pdf.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn renders_multisheet_report_and_referral(){
        let record=serde_json::json!({
            "id":"sample", "patient_name":"Patient Example",
            "diagnosis":"Postoperative evaluation","protocol":"Rehab plan",
            "load_restrictions":"Limited","rom_restrictions":"Flexion 60 degrees",
            "narrative":"Detailed rehabilitation report. ".repeat(700),
            "clinical_summary_json":"{}","version":3,
            "status":"final","author_id":"doctor-test"
        });
        for kind in ["referral","report"]{
            let bytes=render(&record,"Test Clinic",kind).unwrap();
            assert!(bytes.starts_with(b"%PDF-"));
            assert!(bytes.windows(5).any(|w|w==b"%%EOF"));
            assert!(bytes.len()>1000);
        }
    }
}
