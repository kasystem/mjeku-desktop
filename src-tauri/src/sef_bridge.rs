//! Klienti i urës SEF me PROGRAMERI POS.
//!
//! Përdoret VETËM kur motori fiskal (`fiscal_printer_provider`) është
//! `sef_programeri` ("SEF (PROGRAMERI POS)"). ENTERNET, Global EU dhe vlera e
//! vjetër `sef` ("SEF (Printer Windows)", printim PDF) nuk kalojnë kurrë këtu.
//!
//! Kontrata: `pos-native-windows/docs/MJEKU_SEF_BRIDGE_CONTRACT.md` (v1).
//! - Windows: named pipe `\\.\pipe\<PROGRAMERI_FISCAL_PIPE>`, çdo mesazh =
//!   4 bajt gjatësi little-endian + JSON UTF-8 (maks. 1 MiB).
//! - Android: shërbim i lidhur (bound service) — ende i paimplementuar (TODO).
//! - macOS/iOS: nuk mbështetet; zgjedhja SEF (PROGRAMERI POS) nuk shfaqet.
//!
//! Paratë dërgohen gjithmonë si cent (i64). Rregullat e konvertimit janë te
//! `money_to_cents`, `line_total_cents` dhe `build_fiscalize_request`.

// Transporti real ekziston vetëm në Windows; në platformat e tjera disa ndihmës mbeten pa përdorim.
#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

/// Vlera e ruajtur te `fiscal_printer_provider` për motorin SEF përmes PROGRAMERI POS.
/// E veçantë nga `sef` ekzistues (printim PDF në printer Windows), që mbetet i pandryshuar.
pub const PROVIDER_VALUE: &str = "sef_programeri";

pub const ENV_PIPE: &str = "PROGRAMERI_FISCAL_PIPE";
pub const ENV_TOKEN: &str = "PROGRAMERI_FISCAL_TOKEN";
pub const ENV_CONTEXT: &str = "PROGRAMERI_FISCAL_CONTEXT";

pub const PROTOCOL_VERSION: u64 = 1;
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;
pub const SOURCE: &str = "mjeku";

/// Kohëzgjatja për thirrjet e zakonshme.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
/// `configure` hap magjistarin SEF dhe pret derisa përdoruesi ta mbyllë.
/// Nuk ka kufi praktik; 8 orë vetëm që një lidhje e humbur të mos mbetet përgjithmonë.
pub const CONFIGURE_TIMEOUT: Duration = Duration::from_secs(8 * 60 * 60);

pub const MSG_NOT_LAUNCHED: &str = "Hape Mjekun nga PROGRAMERI POS.";
pub const MSG_SWITCH_BACK_REFUSED: &str = "Kjo arkë punon me SEF (PROGRAMERI POS). Kthimi te ENTERNET, Global EU ose SEF (Printer Windows) nuk lejohet, sepse e njëjta pikë shitjeje nuk mund të bëjë edhe PEF edhe SEF. Për ndryshim kontakto mbështetjen e PROGRAMERI POS.";
pub const MSG_SELECT_VIA_CONFIGURE: &str = "SEF (PROGRAMERI POS) zgjidhet me butonin \"Konfiguro SEF\" te Cilësimet; motori ruhet vetëm pasi konfigurimi SEF të përfundojë.";
pub const MSG_CONFIGURE_NOT_READY: &str =
    "Konfigurimi SEF nuk përfundoi. Motori fiskal mbeti i pandryshuar.";
pub const MSG_UNSUPPORTED_PLATFORM: &str =
    "SEF (PROGRAMERI POS) punon vetëm në Windows dhe Android.";

/// True vetëm në platformat ku SEF (PROGRAMERI POS) lejohet (Windows, Android).
pub const fn platform_supported() -> bool {
    cfg!(any(target_os = "windows", target_os = "android"))
}

/// Aftësia që `get_app_info` raporton te UI-ja. UI-ja (paketa që përditësohet veç)
/// shfaq zgjedhjen SEF (PROGRAMERI POS), "Kuponët" dhe banerin SEF VETËM kur kjo
/// aftësi ekziston — kështu një UI e re mbi një binar të vjetër duket si sot.
pub const CAPABILITY_BRIDGE_V1: &str = "sef_programeri_bridge_v1";

/// Lista e aftësive të binarit. Në macOS/iOS bosh (SEF nuk shfaqet fare).
pub fn capabilities() -> Vec<String> {
    if platform_supported() {
        vec![CAPABILITY_BRIDGE_V1.to_string()]
    } else {
        Vec::new()
    }
}

// ---------------------------------------------------------------------------
// Gabimet
// ---------------------------------------------------------------------------

/// Gabim i urës. `message` është gjithmonë tekst shqip për përdoruesin.
/// Kodet nga serveri janë ato të kontratës (`VALIDATION`, `SEF_LOCKED_48H`, ...);
/// kodet lokale: `NOT_LAUNCHED`, `NOT_RUNNING`, `TRANSPORT`, `TIMEOUT`,
/// `PROTOCOL`, `UNSUPPORTED_PLATFORM`, `LOCAL_VALIDATION`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BridgeError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl BridgeError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
            details: None,
        }
    }
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for BridgeError {}

// ---------------------------------------------------------------------------
// Seanca (variablat e mjedisit që vendos PROGRAMERI POS)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Session {
    pub pipe: String,
    token: String,
    pub context: Option<String>,
}

impl Session {
    pub fn from_env() -> Result<Self, BridgeError> {
        Self::from_values(
            std::env::var(ENV_PIPE).ok(),
            std::env::var(ENV_TOKEN).ok(),
            std::env::var(ENV_CONTEXT).ok(),
        )
    }

    pub fn from_values(
        pipe: Option<String>,
        token: Option<String>,
        context: Option<String>,
    ) -> Result<Self, BridgeError> {
        let pipe = pipe.map(|s| s.trim().to_string()).unwrap_or_default();
        let token = token.map(|s| s.trim().to_string()).unwrap_or_default();
        if pipe.is_empty() || token.is_empty() {
            return Err(BridgeError::new("NOT_LAUNCHED", MSG_NOT_LAUNCHED));
        }
        if !is_valid_pipe_name(&pipe) {
            return Err(BridgeError::new("NOT_LAUNCHED", MSG_NOT_LAUNCHED));
        }
        let context = context
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        Ok(Self {
            pipe,
            token,
            context,
        })
    }
}

/// Vetëm emër i thjeshtë pipe-i lokal: pa `\`, `/`, `..`, pa emër serveri.
pub fn is_valid_pipe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && !name.contains("..")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

/// True kur Mjeku është nisur nga PROGRAMERI POS (variablat ekzistojnë).
pub fn launched_from_programeri() -> bool {
    Session::from_env().is_ok()
}

pub fn context_id() -> Option<String> {
    Session::from_env().ok().and_then(|s| s.context)
}

// ---------------------------------------------------------------------------
// Kornizimi dhe mbështjellja (envelope)
// ---------------------------------------------------------------------------

/// 4 bajt gjatësi (u32 little-endian) + payload.
pub fn encode_frame(payload: &[u8]) -> Result<Vec<u8>, BridgeError> {
    if payload.len() > MAX_FRAME_BYTES {
        return Err(BridgeError::new(
            "LOCAL_VALIDATION",
            "Mesazhi për PROGRAMERI POS është shumë i madh.",
        ));
    }
    let len = payload.len() as u32;
    let mut out = Vec::with_capacity(4 + payload.len());
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(payload);
    Ok(out)
}

pub fn decode_frame_len(header: [u8; 4]) -> Result<usize, BridgeError> {
    let len = u32::from_le_bytes(header) as usize;
    if len > MAX_FRAME_BYTES {
        return Err(protocol_error());
    }
    Ok(len)
}

fn protocol_error() -> BridgeError {
    BridgeError::new(
        "PROTOCOL",
        "Përgjigje e pavlefshme nga PROGRAMERI POS. Mbyll Mjekun dhe hape përsëri nga PROGRAMERI POS.",
    )
}

pub fn build_request(id: &str, token: &str, op: &str, params: Value) -> Value {
    json!({
        "v": PROTOCOL_VERSION,
        "id": id,
        "token": token,
        "op": op,
        "params": params,
    })
}

/// Kthen `result` (ose `{}`) kur `ok:true`; përndryshe gabimin e serverit me mesazhin shqip.
pub fn parse_response(expected_id: &str, body: &[u8]) -> Result<Value, BridgeError> {
    let v: Value = serde_json::from_slice(body).map_err(|_| protocol_error())?;
    if v.get("v").and_then(|x| x.as_u64()) != Some(PROTOCOL_VERSION) {
        return Err(protocol_error());
    }
    if v.get("id").and_then(|x| x.as_str()) != Some(expected_id) {
        return Err(protocol_error());
    }
    match v.get("ok").and_then(|x| x.as_bool()) {
        Some(true) => Ok(v.get("result").cloned().unwrap_or_else(|| json!({}))),
        Some(false) => {
            let err = v.get("error").cloned().unwrap_or_else(|| json!({}));
            let code = err
                .get("code")
                .and_then(|x| x.as_str())
                .unwrap_or("INTERNAL")
                .to_string();
            let message = err
                .get("message")
                .and_then(|x| x.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| format!("PROGRAMERI POS ktheu gabim ({code})."));
            Err(BridgeError {
                code,
                message,
                details: err.get("details").cloned().filter(|d| !d.is_null()),
            })
        }
        None => Err(protocol_error()),
    }
}

// ---------------------------------------------------------------------------
// Thirrja
// ---------------------------------------------------------------------------

/// Dërgon një kërkesë dhe pret përgjigjen. Lidhja hapet për çdo thirrje
/// (serveri pranon rilidhje brenda seancës).
pub async fn call(op: &str, params: Value, timeout: Duration) -> Result<Value, BridgeError> {
    if !platform_supported() {
        return Err(BridgeError::new(
            "UNSUPPORTED_PLATFORM",
            MSG_UNSUPPORTED_PLATFORM,
        ));
    }
    let session = Session::from_env()?;
    call_with_session(&session, op, params, timeout).await
}

pub async fn call_with_session(
    session: &Session,
    op: &str,
    params: Value,
    timeout: Duration,
) -> Result<Value, BridgeError> {
    let id = uuid::Uuid::new_v4().to_string();
    let req = build_request(&id, &session.token, op, params);
    let bytes = serde_json::to_vec(&req).map_err(|_| protocol_error())?;
    let frame = encode_frame(&bytes)?;
    let fut = transport::roundtrip(&session.pipe, frame);
    let body = match tokio::time::timeout(timeout, fut).await {
        Ok(r) => r?,
        Err(_) => {
            return Err(BridgeError::new(
                "TIMEOUT",
                "PROGRAMERI POS nuk u përgjigj në kohë. Kontrollo listën \"Kuponët\" para se ta provosh përsëri (e njëjta faturë nuk krijon kupon të dytë).",
            ))
        }
    };
    parse_response(&id, &body)
}

#[cfg(target_os = "windows")]
mod transport {
    use super::{decode_frame_len, BridgeError};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::windows::named_pipe::ClientOptions;

    const ERROR_PIPE_BUSY: i32 = 231;

    fn not_running() -> BridgeError {
        BridgeError::new(
            "NOT_RUNNING",
            "Shërbimi fiskal i PROGRAMERI POS nuk po përgjigjet. Mbyll Mjekun dhe hape përsëri nga PROGRAMERI POS.",
        )
    }

    fn transport_error() -> BridgeError {
        BridgeError::new(
            "TRANSPORT",
            "Lidhja me PROGRAMERI POS u ndërpre. Kontrollo listën \"Kuponët\" dhe provoje përsëri.",
        )
    }

    pub async fn roundtrip(pipe: &str, frame: Vec<u8>) -> Result<Vec<u8>, BridgeError> {
        let full = format!(r"\\.\pipe\{pipe}");
        let started = std::time::Instant::now();
        // ClientOptions përdor si parazgjedhje SECURITY_IDENTIFICATION (serveri
        // mund ta identifikojë thirrësin, por jo ta imitojë).
        let mut client = loop {
            match ClientOptions::new().open(&full) {
                Ok(c) => break c,
                Err(e)
                    if e.raw_os_error() == Some(ERROR_PIPE_BUSY)
                        && started.elapsed() < std::time::Duration::from_secs(5) =>
                {
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(not_running()),
                Err(_) => return Err(not_running()),
            }
        };
        client
            .write_all(&frame)
            .await
            .map_err(|_| transport_error())?;
        client.flush().await.map_err(|_| transport_error())?;
        let mut header = [0u8; 4];
        client
            .read_exact(&mut header)
            .await
            .map_err(|_| transport_error())?;
        let len = decode_frame_len(header)?;
        let mut body = vec![0u8; len];
        client
            .read_exact(&mut body)
            .await
            .map_err(|_| transport_error())?;
        Ok(body)
    }
}

#[cfg(target_os = "android")]
mod transport {
    use super::BridgeError;

    // TODO(Android, Faza 4): transporti është shërbim i lidhur (bound service) te
    // PROGRAMERI POS Mobile, i mbrojtur me leje dhe kontroll nënshkrimi. Kërkon
    // plugin Tauri mobile (Kotlin) që e lidh shërbimin dhe kalon të njëjtat
    // mesazhe JSON. Deri atëherë thirrjet refuzohen dhe asgjë nuk fiskalizohet.
    pub async fn roundtrip(_pipe: &str, _frame: Vec<u8>) -> Result<Vec<u8>, BridgeError> {
        Err(BridgeError::new(
            "UNSUPPORTED_PLATFORM",
            "SEF (PROGRAMERI POS) në Android nuk është ende i lidhur.",
        ))
    }
}

#[cfg(not(any(target_os = "windows", target_os = "android")))]
mod transport {
    use super::{BridgeError, MSG_UNSUPPORTED_PLATFORM};

    pub async fn roundtrip(_pipe: &str, _frame: Vec<u8>) -> Result<Vec<u8>, BridgeError> {
        Err(BridgeError::new(
            "UNSUPPORTED_PLATFORM",
            MSG_UNSUPPORTED_PLATFORM,
        ))
    }
}

// ---------------------------------------------------------------------------
// Paratë, sasitë, TVSH
// ---------------------------------------------------------------------------

fn local_validation(msg: impl Into<String>) -> BridgeError {
    BridgeError::new("LOCAL_VALIDATION", msg)
}

/// Euro (REAL) → cent, rrumbullakim "half away from zero".
/// Shtohet një epsilon i vogël që p.sh. 2.675 (që në f64 është 2.67499999…) të
/// japë 268 si në llogarinë me dorë.
pub fn money_to_cents(v: f64) -> Result<i64, BridgeError> {
    if !v.is_finite() || v.abs() > 1.0e12 {
        return Err(local_validation("Shumë e pavlefshme në faturë."));
    }
    let scaled = v * 100.0;
    let nudged = scaled + scaled.signum() * 1.0e-6;
    Ok(nudged.round() as i64)
}

/// Four-decimal euro unit price, represented exactly in ten-thousandths.
pub fn money_to_ten_thousandths(v: f64) -> Result<i64, BridgeError> {
    if !v.is_finite() || v < 0.0 || v > 100_000_000.0 {
        return Err(local_validation("Çmimi për njësi nuk është i vlefshëm."));
    }
    let scaled=v*10_000.0;
    if (scaled-scaled.round()).abs()>0.00001 {
        return Err(local_validation("Çmimi për njësi lejon maksimumi katër shifra pas presjes."));
    }
    Ok((scaled+0.000001).round() as i64)
}

pub fn line_total_cents_precise(qty_milli: i64, unit_price_ten_thousandths: i64) -> i64 {
    let value=qty_milli as i128 * unit_price_ten_thousandths as i128;
    ((value+50_000)/100_000) as i64
}

/// Sasia → mijëshe (3 decimale), rrumbullakim half away from zero.
pub fn qty_to_milli(q: f64) -> i64 {
    let scaled = q * 1000.0;
    (scaled + scaled.signum() * 1.0e-6).round() as i64
}

/// Mijëshe → tekst dhjetor pa zero në fund: 1000 → "1", 500 → "0.5", 1250 → "1.25".
pub fn qty_string(milli: i64) -> String {
    let neg = milli < 0;
    let m = milli.unsigned_abs();
    let int = m / 1000;
    let frac = m % 1000;
    let mut s = if frac == 0 {
        int.to_string()
    } else {
        let f = format!("{frac:03}");
        format!("{int}.{}", f.trim_end_matches('0'))
    };
    if neg {
        s.insert(0, '-');
    }
    s
}

/// Totali i rreshtit në cent = sasia × çmimi për njësi (cent), rrumbullakim half
/// away from zero kur sasia ka decimale. Llogaritet me numra të plotë.
pub fn line_total_cents(qty_milli: i64, unit_cents: i64) -> i64 {
    let p = (qty_milli as i128) * (unit_cents as i128);
    let q = if p >= 0 {
        (p + 500) / 1000
    } else {
        (p - 500) / 1000
    };
    q as i64
}

/// Përqindjet e TVSH-së për kodet e Mjekut. Parazgjedhjet ndjekin etiketat e
/// Mjekut (Shërbimet/Cilësimet): A = 0% e përjashtuar, C = 0% (jo në TVSH),
/// D = 8%, E = 18%. Mund të ndryshohen me cilësimet `sef_vat_rate_a/c/d/e`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VatRates {
    pub a: u32,
    pub c: u32,
    pub d: u32,
    pub e: u32,
}

impl Default for VatRates {
    fn default() -> Self {
        Self {
            a: 0,
            c: 0,
            d: 8,
            e: 18,
        }
    }
}

pub const VAT_RATE_SETTING_KEYS: [&str; 4] = [
    "sef_vat_rate_a",
    "sef_vat_rate_c",
    "sef_vat_rate_d",
    "sef_vat_rate_e",
];

impl VatRates {
    /// Lexon mbishkrimet nga cilësimet; vlerë bosh/e pavlefshme → parazgjedhja.
    pub fn from_settings(get: impl Fn(&str) -> Option<String>) -> Self {
        let d = Self::default();
        let parse = |key: &str, def: u32| -> u32 {
            get(key)
                .and_then(|v| v.trim().trim_end_matches('%').trim().parse::<u32>().ok())
                .filter(|v| *v <= 100)
                .unwrap_or(def)
        };
        Self {
            a: parse(VAT_RATE_SETTING_KEYS[0], d.a),
            c: parse(VAT_RATE_SETTING_KEYS[1], d.c),
            d: parse(VAT_RATE_SETTING_KEYS[2], d.d),
            e: parse(VAT_RATE_SETTING_KEYS[3], d.e),
        }
    }

    /// Kodi i panjohur trajtohet si C, njësoj si `fiscal_vat_group_for_code` në db.rs.
    pub fn rate_for_code(&self, code: &str) -> u32 {
        match code.trim().to_ascii_uppercase().as_str() {
            "A" => self.a,
            "D" => self.d,
            "E" => self.e,
            _ => self.c,
        }
    }
}

/// Metoda e pagesës së Mjekut → metoda e kontratës.
pub fn map_payment_method(m: &str) -> &'static str {
    match m.trim().to_ascii_lowercase().as_str() {
        "cash" | "" => "cash",
        "card" => "card",
        "bank" | "transfer" => "transfer",
        _ => "other",
    }
}

fn sanitize_title(v: &str) -> String {
    let s = v.replace(['\r', '\n'], " ");
    let s = s.trim();
    if s.is_empty() {
        return "Sherbim".to_string();
    }
    s.chars().take(120).collect()
}

// ---------------------------------------------------------------------------
// Ndërtimi i `fiscalize`
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SaleLine {
    pub id: String,
    pub title: String,
    pub qty: f64,
    pub unit_price: f64,
    pub vat_code: String,
}

#[derive(Debug, Clone)]
pub struct SalePayment {
    pub method: String,
    pub amount: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FiscalItem {
    pub id: String,
    pub title: String,
    pub qty: String,
    pub unit_price_cents: i64,
    pub unit_price_ten_thousandths: i64,
    pub vat_rate_percent: u32,
    #[serde(default)]
    pub category_code: String,
    #[serde(default)]
    pub unit_code: String,
    #[serde(default)]
    pub tax_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FiscalPayment {
    pub method: String,
    pub amount_cents: i64,
}

#[derive(Debug, Clone)]
pub struct FiscalizeRequest {
    pub items: Vec<FiscalItem>,
    pub payments: Vec<FiscalPayment>,
    pub total_cents: i64,
}

/// Ndërton artikujt dhe pagesat.
///
/// Rreshtat: sasia e pavlefshme (≤0, NaN) → 1 dhe çmimi i pavlefshëm (<0, NaN)
/// → 0, njësoj si rruga ENTERNET (`fiscal_receipt_generate_inp`). Çmimi për njësi
/// → cent (half away from zero); totali i rreshtit = `line_total_cents`.
///
/// Pagesat (shuma e tyre DUHET të jetë = shuma e artikujve):
/// 1. Pagesat e shitjes (jo të fshira, shumë > 0) grupohen sipas metodës. Nëse
///    shuma e tyre në cent është saktësisht totali i artikujve → përdoren ashtu.
/// 2. Përndryshe (pagesë e pjesshme, shitje e përzier fiskale/jo-fiskale, pagesë
///    më e madhe): një pagesë e vetme me totalin e artikujve, me metodën e
///    pagesës më të madhe të shitjes.
/// 3. Pa pagesa: një pagesë e vetme `cash` me totalin (si pagesa automatike
///    "[AUTO-FISCAL]" që Mjeku ruan sot pas fiskalizimit).
pub fn build_fiscalize_request(
    lines: &[SaleLine],
    payments: &[SalePayment],
    rates: &VatRates,
) -> Result<FiscalizeRequest, BridgeError> {
    if lines.is_empty() {
        return Err(local_validation("nuk ka rreshta fiskal pa fiskalizuar"));
    }
    let mut items = Vec::with_capacity(lines.len());
    let mut total: i64 = 0;
    for ln in lines {
        let qty = if ln.qty.is_finite() && ln.qty > 0.0 {
            ln.qty
        } else {
            1.0
        };
        let unit_price = if ln.unit_price.is_finite() && ln.unit_price >= 0.0 {
            ln.unit_price
        } else {
            0.0
        };
        let mut qty_milli = qty_to_milli(qty);
        if qty_milli <= 0 {
            qty_milli = 1000;
        }
        let unit_cents = money_to_cents(unit_price)?;
        total += line_total_cents(qty_milli, unit_cents);
        items.push(FiscalItem {
            id: ln.id.clone(),
            title: sanitize_title(&ln.title),
            qty: qty_string(qty_milli),
            unit_price_cents: unit_cents,
            unit_price_ten_thousandths: unit_cents * 100,
            vat_rate_percent: rates.rate_for_code(&ln.vat_code),
            category_code: "SM".into(),
            unit_code: "EA".into(),
            tax_code: ln.vat_code.trim().to_ascii_uppercase(),
        });
    }

    let mut grouped: Vec<FiscalPayment> = Vec::new();
    let mut largest: Option<(i64, &'static str)> = None;
    for p in payments {
        if !p.amount.is_finite() || p.amount <= 0.0 {
            continue;
        }
        let cents = money_to_cents(p.amount)?;
        if cents <= 0 {
            continue;
        }
        let method = map_payment_method(&p.method);
        match largest {
            Some((c, _)) if cents <= c => {}
            _ => largest = Some((cents, method)),
        }
        if let Some(g) = grouped.iter_mut().find(|g| g.method == method) {
            g.amount_cents += cents;
        } else {
            grouped.push(FiscalPayment {
                method: method.to_string(),
                amount_cents: cents,
            });
        }
    }
    let paid: i64 = grouped.iter().map(|g| g.amount_cents).sum();
    let out_payments = if !grouped.is_empty() && paid == total {
        grouped
    } else {
        let method = largest.map(|(_, m)| m).unwrap_or("cash");
        vec![FiscalPayment {
            method: method.to_string(),
            amount_cents: total,
        }]
    };
    let req = FiscalizeRequest {
        items,
        payments: out_payments,
        total_cents: total,
    };
    check_sums(&req)?;
    Ok(req)
}

/// ATK-facing path: do not silently fabricate a cash payment, infer VAT from an
/// unknown code, or replace invalid amounts. Legacy builder is retained only for
/// compatibility tests/older non-SEF paths; the production bridge uses this guard.
pub fn build_fiscalize_request_strict(
    lines: &[SaleLine], payments: &[SalePayment], rates: &VatRates,
) -> Result<FiscalizeRequest, BridgeError> {
    if lines.is_empty() || payments.is_empty() {
        return Err(local_validation("Regjistro pagesën reale dhe shërbimin përpara fiskalizimit SEF."));
    }
    let mut checked_total: i128 = 0;
    for line in lines {
        if line.id.trim().is_empty() || line.title.trim().is_empty() ||
           !line.qty.is_finite() || line.qty <= 0.0 ||
           !line.unit_price.is_finite() || line.unit_price < 0.0 ||
           line.qty > 9_999_999.999 || line.unit_price > 100_000_000.0 ||
           !matches!(line.vat_code.trim().to_ascii_uppercase().as_str(),"A"|"C"|"D"|"E") {
            return Err(local_validation("Artikulli ka sasi, çmim ose kod TVSH të pavlefshëm; korrigjo të dhënat para SEF."));
        }
        if (line.qty*1000.0 - (line.qty*1000.0).round()).abs() > 0.000001 {
            return Err(local_validation("Sasia fiskale lejon maksimumi tri shifra pas presjes."));
        }
        let milli=qty_to_milli(line.qty) as i128;
        let price4=money_to_ten_thousandths(line.unit_price)? as i128;
        checked_total+=(milli*price4+50_000)/100_000;
        if checked_total > i64::MAX as i128 {
            return Err(local_validation("Shuma e faturës tejkalon kufirin e sigurt SEF."));
        }
    }
    let mut paid=0i64;
    for payment in payments {
        if !payment.amount.is_finite() || payment.amount<=0.0 ||
           !matches!(payment.method.trim().to_ascii_lowercase().as_str(),
               "cash"|"card"|"bank"|"transfer") {
            return Err(local_validation("Mënyra ose shuma e pagesës është e pavlefshme për SEF."));
        }
        paid=paid.checked_add(money_to_cents(payment.amount)?)
            .ok_or_else(||local_validation("Shuma e pagesave është shumë e madhe."))?;
    }
    let mut req=build_fiscalize_request(lines,payments,rates)?;
    for (item,line) in req.items.iter_mut().zip(lines.iter()) {
        item.unit_price_ten_thousandths=money_to_ten_thousandths(line.unit_price)?;
    }
    req.total_cents=checked_total as i64;
    // Never use the legacy builder's fallback payment. Reconstruct actual payments.
    req.payments.clear();
    for payment in payments {
        let cents=money_to_cents(payment.amount)?;
        let method=map_payment_method(&payment.method);
        if let Some(existing)=req.payments.iter_mut().find(|x|x.method==method) {
            existing.amount_cents=existing.amount_cents.checked_add(cents)
                .ok_or_else(||local_validation("Shuma e pagesave është shumë e madhe."))?;
        } else {
            req.payments.push(FiscalPayment { method:method.to_string(),amount_cents:cents });
        }
    }
    let coupon_total=money_to_cents(lines.iter().map(|l|l.qty*l.unit_price).sum())?;
    if paid!=req.total_cents || coupon_total!=req.total_cents {
        return Err(local_validation(
            "Pagesat ose rrumbullakimi i kuponit nuk përputhen me shumën e shërbimeve. SEF nuk ndryshon pagesat ose çmimet automatikisht."
        ));
    }
    check_sums(&req)?;
    if req.total_cents<=0 {
        return Err(local_validation("Shuma e faturës SEF duhet të jetë pozitive."));
    }
    Ok(req)
}

/// Kontrolli i kontratës: shuma e artikujve = shuma e pagesave = totali.
pub fn check_sums(req: &FiscalizeRequest) -> Result<(), BridgeError> {
    let items: i64 = req
        .items
        .iter()
        .map(|i| {
            let milli = parse_qty_milli(&i.qty).unwrap_or(0);
            line_total_cents_precise(milli, if i.unit_price_ten_thousandths>0 { i.unit_price_ten_thousandths } else { i.unit_price_cents*100 })
        })
        .sum();
    let pays: i64 = req.payments.iter().map(|p| p.amount_cents).sum();
    if items != pays || items != req.total_cents {
        return Err(local_validation(
            "Shuma e artikujve nuk përputhet me pagesat. Fatura nuk u fiskalizua.",
        ));
    }
    Ok(())
}

fn parse_qty_milli(s: &str) -> Option<i64> {
    let (int, frac) = match s.split_once('.') {
        Some((i, f)) => (i, f),
        None => (s, ""),
    };
    if frac.len() > 3 {
        return None;
    }
    let i: i64 = int.parse().ok()?;
    let f: i64 = if frac.is_empty() {
        0
    } else {
        format!("{frac:0<3}").parse().ok()?
    };
    Some(i * 1000 + if i < 0 { -f } else { f })
}

pub fn fiscalize_params(sale_id: &str, issued_at: &str, req: &FiscalizeRequest) -> Value {
    // `buyer` nuk dërgohet: pacientët janë persona fizikë dhe kontrata e lejon pa blerës.
    json!({
        "source": SOURCE,
        "saleId": sale_id,
        "issuedAt": issued_at,
        "items": req.items,
        "payments": req.payments,
    })
}

/// Explicit fiscal catalog + trusted operator contract. Older fiscalize_params remains
/// available for backward compatibility tests only; SEF issue path uses this version.
pub fn fiscalize_params_v2(sale_id: &str, issued_at: &str, operator_name: &str, req: &FiscalizeRequest) -> Result<Value, BridgeError> {
    if operator_name.trim().is_empty() ||
       req.items.iter().any(|i| !matches!(i.category_code.as_str(),"SM"|"BM"|"PM") ||
           !matches!(i.unit_code.as_str(),"EA"|"HUR"|"DAY") ||
           !matches!(i.tax_code.as_str(),"A"|"C"|"D"|"E")) {
        return Err(local_validation("Konfigurimi i operatorit ose klasifikimi fiskal ATK mungon."));
    }
    Ok(json!({
        "payloadSchemaVersion": 2,
        "operatorName": operator_name.trim(),
        "source": SOURCE,
        "saleId": sale_id,
        "issuedAt": issued_at,
        "items": req.items,
        "payments": req.payments,
    }))
}

/// Rezultati i `fiscalize`, `coupon.return`, `coupon.cancel`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CouponResult {
    pub coupon_id: String,
    #[serde(default)]
    pub daily_number: Option<i64>,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub total_cents: Option<i64>,
    #[serde(default)]
    pub verification_url: Option<String>,
    #[serde(default)]
    pub duplicate: Option<bool>,
}

pub fn parse_coupon_result(v: Value) -> Result<CouponResult, BridgeError> {
    let r: CouponResult = serde_json::from_value(v).map_err(|_| protocol_error())?;
    if r.coupon_id.trim().is_empty() {
        return Err(protocol_error());
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_roundtrip_little_endian() {
        let f = encode_frame(b"{\"a\":1}").unwrap();
        assert_eq!(&f[..4], &[7, 0, 0, 0]);
        assert_eq!(decode_frame_len([f[0], f[1], f[2], f[3]]).unwrap(), 7);
        assert_eq!(&f[4..], b"{\"a\":1}");
        let big = encode_frame(&vec![b'x'; 300]).unwrap();
        assert_eq!(&big[..4], &[0x2c, 0x01, 0, 0]);
    }

    #[test]
    fn frame_limits() {
        assert!(encode_frame(&vec![0u8; MAX_FRAME_BYTES]).is_ok());
        assert!(encode_frame(&vec![0u8; MAX_FRAME_BYTES + 1]).is_err());
        let too_big = ((MAX_FRAME_BYTES + 1) as u32).to_le_bytes();
        assert_eq!(decode_frame_len(too_big).unwrap_err().code, "PROTOCOL");
    }

    #[test]
    fn request_envelope_shape() {
        let r = build_request("id1", "tok", "status", json!({}));
        assert_eq!(r["v"], 1);
        assert_eq!(r["id"], "id1");
        assert_eq!(r["token"], "tok");
        assert_eq!(r["op"], "status");
        assert!(r["params"].is_object());
    }

    #[test]
    fn response_ok_and_error() {
        let ok = br#"{"v":1,"id":"a","ok":true,"result":{"ready":true}}"#;
        assert_eq!(parse_response("a", ok).unwrap()["ready"], true);
        let err = br#"{"v":1,"id":"a","ok":false,"error":{"code":"SEF_LOCKED_48H","message":"SEF eshte bllokuar.","details":{}}}"#;
        let e = parse_response("a", err).unwrap_err();
        assert_eq!(e.code, "SEF_LOCKED_48H");
        assert_eq!(e.message, "SEF eshte bllokuar.");
        // id i gabuar / version i gabuar / jo JSON
        assert_eq!(parse_response("b", ok).unwrap_err().code, "PROTOCOL");
        let v2 = br#"{"v":2,"id":"a","ok":true,"result":{}}"#;
        assert_eq!(parse_response("a", v2).unwrap_err().code, "PROTOCOL");
        assert_eq!(parse_response("a", b"nope").unwrap_err().code, "PROTOCOL");
        // gabim pa mesazh → mesazh rezervë shqip
        let bare = br#"{"v":1,"id":"a","ok":false,"error":{"code":"INTERNAL"}}"#;
        assert!(parse_response("a", bare)
            .unwrap_err()
            .message
            .contains("INTERNAL"));
    }

    #[test]
    fn session_requires_env_values() {
        let e = Session::from_values(None, Some("t".into()), None).unwrap_err();
        assert_eq!(e.message, MSG_NOT_LAUNCHED);
        assert!(Session::from_values(Some("p".into()), Some("  ".into()), None).is_err());
        assert!(Session::from_values(Some(r"..\x".into()), Some("t".into()), None).is_err());
        assert!(Session::from_values(Some(r"srv\pipe\x".into()), Some("t".into()), None).is_err());
        let s = Session::from_values(
            Some("programeri-pos-fiscal-3f2a".into()),
            Some("tok".into()),
            Some("".into()),
        )
        .unwrap();
        assert_eq!(s.pipe, "programeri-pos-fiscal-3f2a");
        assert!(s.context.is_none());
    }

    #[test]
    fn cents_round_half_away_from_zero() {
        assert_eq!(money_to_cents(25.0).unwrap(), 2500);
        assert_eq!(money_to_cents(0.1 + 0.2).unwrap(), 30);
        assert_eq!(money_to_cents(2.675).unwrap(), 268);
        assert_eq!(money_to_cents(1.005).unwrap(), 101);
        assert_eq!(money_to_cents(0.004).unwrap(), 0);
        assert_eq!(money_to_cents(-2.675).unwrap(), -268);
        assert_eq!(money_to_cents(19.99).unwrap(), 1999);
        assert!(money_to_cents(f64::NAN).is_err());
    }

    #[test]
    fn qty_and_line_totals() {
        assert_eq!(qty_string(qty_to_milli(1.0)), "1");
        assert_eq!(qty_string(qty_to_milli(0.5)), "0.5");
        assert_eq!(qty_string(qty_to_milli(1.25)), "1.25");
        assert_eq!(qty_string(qty_to_milli(2.0005)), "2.001");
        assert_eq!(qty_string(10), "0.01");
        assert_eq!(line_total_cents(1000, 2500), 2500);
        assert_eq!(line_total_cents(500, 333), 167); // 166.5 → 167
        assert_eq!(line_total_cents(333, 100), 33); // 33.3 → 33
        assert_eq!(parse_qty_milli("1.25"), Some(1250));
        assert_eq!(parse_qty_milli("0.5"), Some(500));
        assert_eq!(parse_qty_milli("3"), Some(3000));
    }

    #[test]
    fn vat_mapping_defaults_and_overrides() {
        let r = VatRates::default();
        assert_eq!(r.rate_for_code("A"), 0);
        assert_eq!(r.rate_for_code("c"), 0);
        assert_eq!(r.rate_for_code("D"), 8);
        assert_eq!(r.rate_for_code(" e "), 18);
        assert_eq!(r.rate_for_code("X"), 0); // i panjohur → si C
        let o = VatRates::from_settings(|k| match k {
            "sef_vat_rate_e" => Some("17%".to_string()),
            "sef_vat_rate_d" => Some("abc".to_string()),
            "sef_vat_rate_a" => Some("101".to_string()),
            _ => None,
        });
        assert_eq!(o.e, 17);
        assert_eq!(o.d, 8);
        assert_eq!(o.a, 0);
    }

    fn line(id: &str, qty: f64, price: f64, vat: &str) -> SaleLine {
        SaleLine {
            id: id.into(),
            title: format!("Sherbimi {id}"),
            qty,
            unit_price: price,
            vat_code: vat.into(),
        }
    }

    fn pay(method: &str, amount: f64) -> SalePayment {
        SalePayment {
            method: method.into(),
            amount,
        }
    }

    #[test]
    fn payments_used_when_they_match() {
        let lines = [line("1", 1.0, 20.0, "E"), line("2", 2.0, 2.5, "D")];
        let pays = [pay("cash", 10.0), pay("card", 10.0), pay("cash", 5.0)];
        let r = build_fiscalize_request(&lines, &pays, &VatRates::default()).unwrap();
        assert_eq!(r.total_cents, 2500);
        assert_eq!(r.payments.len(), 2);
        assert_eq!(r.payments[0].method, "cash");
        assert_eq!(r.payments[0].amount_cents, 1500);
        assert_eq!(r.payments[1].method, "card");
        assert_eq!(r.items[0].vat_rate_percent, 18);
        assert_eq!(r.items[1].vat_rate_percent, 8);
        assert_eq!(r.items[1].qty, "2");
    }

    #[test]
    fn single_payment_when_mismatch_or_none() {
        let lines = [line("1", 1.0, 30.0, "C")];
        // pagesë e pjesshme → një pagesë me totalin, metoda e pagesës më të madhe
        let r = build_fiscalize_request(
            &lines,
            &[pay("bank", 20.0), pay("cash", 5.0)],
            &VatRates::default(),
        )
        .unwrap();
        assert_eq!(r.payments.len(), 1);
        assert_eq!(r.payments[0].method, "transfer");
        assert_eq!(r.payments[0].amount_cents, 3000);
        // pa pagesa → cash
        let r = build_fiscalize_request(&lines, &[], &VatRates::default()).unwrap();
        assert_eq!(r.payments[0].method, "cash");
        assert_eq!(r.payments[0].amount_cents, 3000);
        // pagesa negative/zero injorohen
        let r = build_fiscalize_request(
            &lines,
            &[pay("card", 0.0), pay("card", -5.0)],
            &VatRates::default(),
        )
        .unwrap();
        assert_eq!(r.payments[0].method, "cash");
    }

    #[test]
    fn strict_sef_uses_four_decimal_unit_price_without_inventing_payment() {
        let rates=VatRates::default();
        let lines=[line("service",2.0,1.2345,"A")];
        let result=build_fiscalize_request_strict(&lines,&[pay("cash",2.47)],&rates).unwrap();
        assert_eq!(result.items[0].unit_price_ten_thousandths,12345);
        assert_eq!(result.items[0].unit_price_cents,123);
        assert_eq!(result.total_cents,247);
        assert_eq!(fiscalize_params("s","2026-10-26T10:00:00Z",&result)["items"][0]["unitPriceTenThousandths"],12345);
        assert!(build_fiscalize_request_strict(&[line("x",1.0,1.23456,"A")],
            &[pay("cash",1.23)],&rates).is_err());
    }

    #[test]
    fn strict_sef_refuses_invented_cash_unknown_tax_and_partial_payment() {
        let lines=[line("v1",1.0,25.0,"D")];
        let rates=VatRates::default();
        assert!(build_fiscalize_request_strict(&lines,&[],&rates).is_err());
        assert!(build_fiscalize_request_strict(&lines,&[pay("cash",10.0)],&rates).is_err());
        assert!(build_fiscalize_request_strict(&[line("v1",1.0,25.0,"?")],
            &[pay("cash",25.0)],&rates).is_err());
        assert!(build_fiscalize_request_strict(&[line("v1",1.0004,25.0,"D")],
            &[pay("cash",25.0)],&rates).is_err());
        let accepted=build_fiscalize_request_strict(&lines,&[pay("card",25.0)],&rates).unwrap();
        assert_eq!(accepted.total_cents,2500);
        assert_eq!(accepted.payments[0].method,"card");
    }

    #[test]
    fn fractional_items_sum_equals_payments() {
        let lines = [
            line("1", 0.5, 3.33, "A"),
            line("2", 1.333, 0.07, "D"),
            line("3", 3.0, 0.1, "E"),
        ];
        let r =
            build_fiscalize_request(&lines, &[pay("cash", 100.0)], &VatRates::default()).unwrap();
        // 0.5×333=166.5→167, 1.333×7=9.331→9, 3×10=30 → 206
        assert_eq!(r.total_cents, 206);
        assert_eq!(r.payments[0].amount_cents, 206);
        assert!(check_sums(&r).is_ok());
    }

    #[test]
    fn invalid_lines_follow_enternet_rules() {
        let r = build_fiscalize_request(
            &[line("1", 0.0, -4.0, "A"), line("2", f64::NAN, 5.0, "")],
            &[],
            &VatRates::default(),
        )
        .unwrap();
        assert_eq!(r.items[0].qty, "1");
        assert_eq!(r.items[0].unit_price_cents, 0);
        assert_eq!(r.items[1].qty, "1");
        assert_eq!(r.total_cents, 500);
        assert!(build_fiscalize_request(&[], &[], &VatRates::default()).is_err());
    }

    #[test]
    fn check_sums_detects_mismatch() {
        let mut r = build_fiscalize_request(&[line("1", 1.0, 5.0, "A")], &[], &VatRates::default())
            .unwrap();
        r.payments[0].amount_cents += 1;
        assert!(check_sums(&r).is_err());
    }

    #[test]
    fn fiscalize_params_shape() {
        let r = build_fiscalize_request(&[line("vi-1", 1.0, 25.0, "D")], &[], &VatRates::default())
            .unwrap();
        let p = fiscalize_params("sale-1", "2026-10-15T10:00:00+02:00", &r);
        assert_eq!(p["source"], "mjeku");
        assert_eq!(p["saleId"], "sale-1");
        assert_eq!(p["items"][0]["id"], "vi-1");
        assert_eq!(p["items"][0]["qty"], "1");
        assert_eq!(p["items"][0]["unitPriceCents"], 2500);
        assert_eq!(p["items"][0]["vatRatePercent"], 8);
        assert_eq!(p["payments"][0]["amountCents"], 2500);
        assert!(p.get("buyer").is_none());
    }

    #[test]
    fn coupon_result_parsing() {
        let v = json!({"couponId":"C-1","dailyNumber":12,"status":"PENDING_OFFLINE",
            "createdAt":"2026-10-15T10:00:00+02:00","totalCents":2500,"verificationUrl":null,"duplicate":true});
        let r = parse_coupon_result(v).unwrap();
        assert_eq!(r.daily_number, Some(12));
        assert_eq!(r.duplicate, Some(true));
        assert!(parse_coupon_result(json!({"couponId":""})).is_err());
    }

    #[cfg(target_os = "windows")]
    fn test_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    /// Si `sef_call_blocking` te lib.rs: runtime i ri current-thread brenda spawn_blocking.
    #[cfg(target_os = "windows")]
    #[test]
    fn windows_pipe_call_from_spawn_blocking() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::windows::named_pipe::ServerOptions;
        test_runtime().block_on(async {
            let name = format!("mjeku-sef-test-{}", uuid::Uuid::new_v4());
            let mut server = ServerOptions::new()
                .first_pipe_instance(true)
                .create(format!(r"\\.\pipe\{name}"))
                .unwrap();
            let srv = tokio::spawn(async move {
                server.connect().await.unwrap();
                let mut h = [0u8; 4];
                server.read_exact(&mut h).await.unwrap();
                let mut b = vec![0u8; u32::from_le_bytes(h) as usize];
                server.read_exact(&mut b).await.unwrap();
                let req: Value = serde_json::from_slice(&b).unwrap();
                assert_eq!(req["op"], "fiscalize");
                assert_eq!(req["params"]["saleId"], "s1");
                let resp = json!({"v":1,"id":req["id"],"ok":true,"result":{
                    "couponId":"C-7","dailyNumber":7,"status":"FISCALIZED","duplicate":false}});
                let out = encode_frame(&serde_json::to_vec(&resp).unwrap()).unwrap();
                server.write_all(&out).await.unwrap();
            });
            let res = tokio::task::spawn_blocking(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                let session = Session::from_values(Some(name), Some("tok".into()), None).unwrap();
                let req = build_fiscalize_request(
                    &[line("vi-1", 1.0, 25.0, "D")],
                    &[],
                    &VatRates::default(),
                )
                .unwrap();
                let params = fiscalize_params("s1", "2026-10-15T10:00:00+02:00", &req);
                rt.block_on(call_with_session(
                    &session,
                    "fiscalize",
                    params,
                    DEFAULT_TIMEOUT,
                ))
            })
            .await
            .unwrap()
            .unwrap();
            let c = parse_coupon_result(res).unwrap();
            assert_eq!(c.coupon_id, "C-7");
            assert_eq!(c.daily_number, Some(7));
            srv.await.unwrap();
        });
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_pipe_roundtrip_and_errors() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::windows::named_pipe::ServerOptions;
        test_runtime().block_on(async {
            let name = format!("mjeku-sef-test-{}", uuid::Uuid::new_v4());
            let full = format!(r"\\.\pipe\{name}");
            let mut server = ServerOptions::new()
                .first_pipe_instance(true)
                .create(&full)
                .unwrap();
            let srv = tokio::spawn(async move {
                server.connect().await.unwrap();
                let mut h = [0u8; 4];
                server.read_exact(&mut h).await.unwrap();
                let mut b = vec![0u8; u32::from_le_bytes(h) as usize];
                server.read_exact(&mut b).await.unwrap();
                let req: Value = serde_json::from_slice(&b).unwrap();
                assert_eq!(req["v"], 1);
                assert_eq!(req["token"], "tok");
                assert_eq!(req["op"], "status");
                let resp = json!({"v":1,"id":req["id"],"ok":true,"result":{"configured":true}});
                let out = encode_frame(&serde_json::to_vec(&resp).unwrap()).unwrap();
                server.write_all(&out).await.unwrap();
            });
            let session = Session::from_values(Some(name), Some("tok".into()), None).unwrap();
            let r = call_with_session(&session, "status", json!({}), DEFAULT_TIMEOUT)
                .await
                .unwrap();
            assert_eq!(r["configured"], true);
            srv.await.unwrap();

            // Pipe që nuk ekziston → NOT_RUNNING
            let missing = Session::from_values(
                Some(format!("mjeku-sef-missing-{}", uuid::Uuid::new_v4())),
                Some("tok".into()),
                None,
            )
            .unwrap();
            let e = call_with_session(&missing, "status", json!({}), DEFAULT_TIMEOUT)
                .await
                .unwrap_err();
            assert_eq!(e.code, "NOT_RUNNING");

            // Serveri nuk përgjigjet → TIMEOUT
            let name2 = format!("mjeku-sef-test-{}", uuid::Uuid::new_v4());
            let silent = ServerOptions::new()
                .first_pipe_instance(true)
                .create(format!(r"\\.\pipe\{name2}"))
                .unwrap();
            let hold = tokio::spawn(async move {
                silent.connect().await.unwrap();
                tokio::time::sleep(Duration::from_secs(2)).await;
                drop(silent);
            });
            let s2 = Session::from_values(Some(name2), Some("tok".into()), None).unwrap();
            let e = call_with_session(&s2, "status", json!({}), Duration::from_millis(300))
                .await
                .unwrap_err();
            assert_eq!(e.code, "TIMEOUT");
            hold.await.unwrap();
        });
    }

    #[test]
    fn capability_only_on_supported_platforms() {
        let caps = capabilities();
        if cfg!(any(target_os = "windows", target_os = "android")) {
            assert_eq!(caps, vec![CAPABILITY_BRIDGE_V1.to_string()]);
        } else {
            assert!(caps.is_empty());
        }
        assert_eq!(CAPABILITY_BRIDGE_V1, "sef_programeri_bridge_v1");
    }

    #[test]
    fn payment_method_mapping() {
        assert_eq!(map_payment_method("cash"), "cash");
        assert_eq!(map_payment_method("CARD"), "card");
        assert_eq!(map_payment_method("bank"), "transfer");
        assert_eq!(map_payment_method("other"), "other");
        assert_eq!(map_payment_method("cheque"), "other");
        assert_eq!(map_payment_method(""), "cash");
    }
}
