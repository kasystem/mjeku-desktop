// Integrim me analizues laboratorikë (hematologji/biokimi) përmes portit
// serial (RS232), duke përdorur protokollin publik ASTM E1394-97 — standardi
// më i zakonshëm te analizuesit e klasës së klinikave private në rajon.
//
// KUJDES: Profilet e listuara më poshtë (baud rate, data/stop bits) janë
// vlerat DEFAULT të dokumentuara publikisht për ASTM E1394 mbi RS232 (9600
// 8N1) — çdo prodhues mund t'i implementojë me variacione (checksum strict,
// timing i frame-ve). Përpara përdorimit real me një pajisje konkrete, duhet
// verifikuar/rregulluar kundër dokumentit ICD (Interface Control Document)
// të vetë analizuesit — kjo listë s'është testuar kundër hardware real.

use crate::models::AnalyzerProfile;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[cfg(desktop)]
use std::io::Read;
#[cfg(desktop)]
use std::time::Duration;

use crate::db::Db;
use crate::models::LabDeviceStatus;

pub const PROTOCOL_ASTM_E1394: &str = "astm_e1394";

pub fn known_profiles() -> Vec<AnalyzerProfile> {
    let astm_default = |id: &str, brand: &str, model: &str| AnalyzerProfile {
        id: id.to_string(),
        brand: brand.to_string(),
        model: model.to_string(),
        protocol: PROTOCOL_ASTM_E1394.to_string(),
        baud_rate: 9600,
        data_bits: 8,
        parity: "none".to_string(),
        stop_bits: 1,
        notes: "ASTM E1394 mbi RS232, 9600 8N1 (default i dokumentuar publikisht — verifiko kundër ICD-së së pajisjes)".to_string(),
    };

    vec![
        astm_default("mindray_bc_series", "Mindray", "BC-series (hematologji)"),
        astm_default("mindray_bs_series", "Mindray", "BS-series (biokimi)"),
        astm_default("sysmex_xp_100", "Sysmex", "XP-100 (hematologji)"),
        astm_default("sysmex_xs_500i", "Sysmex", "XS-500i (hematologji)"),
        astm_default("erba_xl_640", "ERBA", "XL-640 (biokimi)"),
        astm_default("erba_elite_series", "ERBA", "ELite-series (hematologji)"),
        astm_default("biosystems_a25", "Biosystems", "A25 (biokimi)"),
        astm_default("biosystems_bts_350", "Biosystems", "BTS-350 (biokimi)"),
        astm_default("urit_8021", "Urit", "8021 (hematologji)"),
        astm_default("urit_8160", "Urit", "8160 (biokimi)"),
        astm_default("roche_cobas_c111", "Roche", "Cobas c111 (biokimi)"),
        astm_default("abbott_alinity", "Abbott", "Alinity (biokimi/imunologji)"),
    ]
}

pub fn find_profile(id: &str) -> Option<AnalyzerProfile> {
    known_profiles().into_iter().find(|p| p.id == id)
}

/// Një rezultat i vetëm (një rresht "R" ASTM).
#[derive(Debug, Clone, Default)]
pub struct AstmResult {
    pub test_id: String,
    pub value: String,
    pub units: String,
    pub ref_range: String,
    pub flag: String,
}

#[derive(Debug, Clone, Default)]
pub struct ParsedAstmMessage {
    pub patient_id: String,
    pub patient_name: String,
    pub results: Vec<AstmResult>,
}

const STX: u8 = 0x02;
const ETX: u8 = 0x03;
const ETB: u8 = 0x17;
const EOT: u8 = 0x04;
const ENQ: u8 = 0x05;
const CR: u8 = 0x0D;
const LF: u8 = 0x0A;

/// Heq framing-un e nivelit të ulët ASTM (STX/numri i frame-it/ETX-ETB +
/// checksum + CRLF) nëse është i pranishëm, dhe kthen tekstin e pastër ASCII
/// me rreshtat e ndarë me '\n'. Është tolerant — nëse hyrja s'ka framing
/// (disa bridge serial-to-TCP e heqin vetë), thjesht i pastron kontrollet.
pub fn strip_astm_framing(raw: &[u8]) -> String {
    let mut text_bytes: Vec<u8> = Vec::with_capacity(raw.len());
    let mut i = 0usize;
    while i < raw.len() {
        let b = raw[i];
        match b {
            STX => {
                // Anashkalo STX + numrin e frame-it (1 shifër ASCII).
                i += 1;
                if i < raw.len() && raw[i].is_ascii_digit() {
                    i += 1;
                }
                continue;
            }
            ETX | ETB => {
                // Fundi i frame-it: anashkalo checksum-in (2 karaktere hex) + CRLF.
                i += 1;
                let skip = 2usize.min(raw.len().saturating_sub(i));
                i += skip;
                if i < raw.len() && raw[i] == CR {
                    i += 1;
                }
                if i < raw.len() && raw[i] == LF {
                    i += 1;
                }
                text_bytes.push(b'\n');
                continue;
            }
            EOT | ENQ => {
                i += 1;
                continue;
            }
            CR => {
                text_bytes.push(b'\n');
                i += 1;
            }
            LF => {
                i += 1; // CR tashmë e shtoi '\n'-in; mos e dubliko
            }
            _ => {
                text_bytes.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&text_bytes).to_string()
}

/// Ndan tekstin e pastruar në rreshta ASTM, secili i ndarë me '|' në fusha.
pub fn parse_astm_records(text: &str) -> Vec<Vec<String>> {
    text.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(|l| l.split('|').map(|f| f.to_string()).collect::<Vec<String>>())
        .filter(|fields| {
            fields
                .first()
                .and_then(|f| f.chars().next())
                .map(|c| c.is_ascii_alphabetic())
                .unwrap_or(false)
        })
        .collect()
}

fn record_type(fields: &[String]) -> char {
    fields
        .first()
        .and_then(|f| f.chars().next())
        .unwrap_or(' ')
        .to_ascii_uppercase()
}

/// Formaton emrin e pacientit nga fusha P e ASTM: "mbiemer^emer^mesem" -> "Emer Mbiemer".
fn format_patient_name(raw: &str) -> String {
    let parts: Vec<&str> = raw.split('^').collect();
    let last = parts.first().copied().unwrap_or("").trim();
    let first = parts.get(1).copied().unwrap_or("").trim();
    if first.is_empty() && last.is_empty() {
        String::new()
    } else if first.is_empty() {
        last.to_string()
    } else if last.is_empty() {
        first.to_string()
    } else {
        format!("{first} {last}")
    }
}

pub fn parse_astm_message(text: &str) -> ParsedAstmMessage {
    let records = parse_astm_records(text);
    let mut msg = ParsedAstmMessage::default();

    for fields in &records {
        match record_type(fields) {
            'P' => {
                // P|seq|practice_id|lab_id|patient_id|name|...
                if let Some(pid) = fields.get(3).filter(|s| !s.is_empty()) {
                    msg.patient_id = pid.clone();
                } else if let Some(pid) = fields.get(2).filter(|s| !s.is_empty()) {
                    msg.patient_id = pid.clone();
                }
                if let Some(name_raw) = fields.get(5) {
                    let name = format_patient_name(name_raw);
                    if !name.is_empty() {
                        msg.patient_name = name;
                    }
                }
            }
            'R' => {
                // R|seq|test_id^...|value|units|ref_range|flags|...
                let test_id = fields
                    .get(2)
                    .map(|s| s.split('^').last().unwrap_or(s).to_string())
                    .unwrap_or_default();
                let value = fields.get(3).cloned().unwrap_or_default();
                let units = fields.get(4).cloned().unwrap_or_default();
                let ref_range = fields.get(5).cloned().unwrap_or_default();
                let flag = fields.get(6).cloned().unwrap_or_default();
                if !test_id.is_empty() || !value.is_empty() {
                    msg.results.push(AstmResult {
                        test_id,
                        value,
                        units,
                        ref_range,
                        flag,
                    });
                }
            }
            _ => {}
        }
    }

    msg
}

/// Formaton rezultatet si tekst i lexueshëm shqip, për t'u shtuar te
/// fusha `analyses` e vizitës (ose për t'u shfaqur në listën "të papërputhura").
pub fn format_results_as_text(msg: &ParsedAstmMessage) -> String {
    let mut out = String::new();
    out.push_str("— Rezultate laboratori (import automatik) —\n");
    if !msg.patient_name.is_empty() {
        out.push_str(&format!("Pacienti (nga analizuesi): {}\n", msg.patient_name));
    }
    for r in &msg.results {
        let flag_suffix = if r.flag.trim().is_empty() {
            String::new()
        } else {
            format!(" [{}]", r.flag.trim())
        };
        out.push_str(&format!(
            "{}: {} {}{}{}\n",
            if r.test_id.is_empty() { "?" } else { &r.test_id },
            r.value,
            r.units,
            if r.ref_range.trim().is_empty() {
                String::new()
            } else {
                format!(" (ref: {})", r.ref_range.trim())
            },
            flag_suffix
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_astm_message_without_framing() {
        // Mesazh minimal ASTM, pa framing STX/ETX (shumë bridge serial-to-app
        // e heqin vetë framing-un e nivelit fizik para se t'ia dorëzojnë app-it).
        let raw = "H|\\^&|||MJEKU-LAB|||||||P|1394-97|20260710120000\r\n\
                   P|1||PID123||Doe^John||19900101|M\r\n\
                   O|1|SID1||^^^GLU|R||20260710120000\r\n\
                   R|1|^^^GLU|98|mg/dL|70-110|N||F\r\n\
                   R|2|^^^WBC|11.2|10^3/uL|4.0-10.0|H||F\r\n\
                   L|1|N\r\n";
        let msg = parse_astm_message(raw);
        assert_eq!(msg.patient_id, "PID123");
        assert_eq!(msg.patient_name, "John Doe");
        assert_eq!(msg.results.len(), 2);
        assert_eq!(msg.results[0].test_id, "GLU");
        assert_eq!(msg.results[0].value, "98");
        assert_eq!(msg.results[0].units, "mg/dL");
        assert_eq!(msg.results[1].flag, "H");

        let text = format_results_as_text(&msg);
        assert!(text.contains("GLU: 98 mg/dL"));
        assert!(text.contains("WBC: 11.2 10^3/uL"));
        assert!(text.contains("[H]"));
    }

    #[test]
    fn strips_stx_etx_framing() {
        let mut raw: Vec<u8> = Vec::new();
        raw.push(STX);
        raw.push(b'1');
        raw.extend_from_slice(b"R|1|^^^GLU|98|mg/dL|70-110|N||F");
        raw.push(CR);
        raw.push(ETX);
        raw.push(b'4');
        raw.push(b'2');
        raw.push(CR);
        raw.push(LF);

        let text = strip_astm_framing(&raw);
        let msg = parse_astm_message(&text);
        assert_eq!(msg.results.len(), 1);
        assert_eq!(msg.results[0].test_id, "GLU");
    }
}

/// Menaxheri i lidhjes seriale me analizuesin — vetëm desktop (Windows/macOS),
/// pasi `serialport` s'është i disponueshëm si varësi mobile — struktura
/// ekziston në të dy platformat, por vetëm `connect()` desktop lidhet vërtet
/// me portin serial; versioni mobile kthen gjithmonë gabim nga `connect()`.
pub struct LabSerialManager {
    stop_flag: Arc<AtomicBool>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    status: Arc<Mutex<LabDeviceStatus>>,
}

impl LabSerialManager {
    pub fn new() -> Self {
        Self {
            stop_flag: Arc::new(AtomicBool::new(false)),
            thread: Mutex::new(None),
            status: Arc::new(Mutex::new(LabDeviceStatus {
                connected: false,
                port_name: None,
                profile_id: None,
                last_error: None,
                last_message_at: None,
            })),
        }
    }

    pub fn status(&self) -> LabDeviceStatus {
        self.status.lock().unwrap().clone()
    }

    pub fn disconnect(&self) {
        self.stop_flag.store(true, Ordering::SeqCst);
        if let Some(handle) = self.thread.lock().unwrap().take() {
            let _ = handle.join();
        }
        let mut st = self.status.lock().unwrap();
        st.connected = false;
    }

}

#[cfg(desktop)]
impl LabSerialManager {
    pub fn connect(
        &self,
        db: Arc<Db>,
        port_name: String,
        profile: AnalyzerProfile,
    ) -> anyhow::Result<()> {
        // Ndal lidhjen e mëparshme (nëse ka) para se të hapësh një të re.
        self.disconnect();
        self.stop_flag.store(false, Ordering::SeqCst);

        let port = serialport::new(&port_name, profile.baud_rate)
            .timeout(Duration::from_millis(500))
            .data_bits(match profile.data_bits {
                7 => serialport::DataBits::Seven,
                _ => serialport::DataBits::Eight,
            })
            .parity(match profile.parity.as_str() {
                "even" => serialport::Parity::Even,
                "odd" => serialport::Parity::Odd,
                _ => serialport::Parity::None,
            })
            .stop_bits(match profile.stop_bits {
                2 => serialport::StopBits::Two,
                _ => serialport::StopBits::One,
            })
            .open()
            .map_err(|e| anyhow::anyhow!("hapja e portit serial '{port_name}' dështoi: {e}"))?;

        {
            let mut st = self.status.lock().unwrap();
            st.connected = true;
            st.port_name = Some(port_name.clone());
            st.profile_id = Some(profile.id.clone());
            st.last_error = None;
        }

        let stop_flag = self.stop_flag.clone();
        let status = self.status.clone();
        let profile_id = profile.id.clone();

        let handle = std::thread::spawn(move || {
            let mut port = port;
            let mut buf = [0u8; 1024];
            let mut acc: Vec<u8> = Vec::new();

            while !stop_flag.load(Ordering::SeqCst) {
                match port.read(&mut buf) {
                    Ok(0) => {}
                    Ok(n) => {
                        acc.extend_from_slice(&buf[..n]);
                        // Mesazhi konsiderohet i plotë kur shohim ETX/ETB (fundi i
                        // frame-it fizik) ose një rresht terminator 'L|...' —
                        // disa bridge/analizues s'e dërgojnë framing-un fizik.
                        let has_frame_end = acc.iter().any(|&b| b == ETX || b == ETB);
                        let text_preview = String::from_utf8_lossy(&acc);
                        let has_terminator_record = text_preview
                            .lines()
                            .any(|l| l.trim_start().starts_with('L') && l.contains('|'));

                        if has_frame_end || has_terminator_record {
                            let text = strip_astm_framing(&acc);
                            let msg = parse_astm_message(&text);
                            if !msg.results.is_empty() {
                                let formatted = format_results_as_text(&msg);
                                let patient_ref = if !msg.patient_name.is_empty() {
                                    format!("{} ({})", msg.patient_name, msg.patient_id)
                                } else {
                                    msg.patient_id.clone()
                                };
                                if let Err(e) = db.lab_inbox_insert(
                                    &profile_id,
                                    &patient_ref,
                                    &formatted,
                                    &text,
                                ) {
                                    let mut st = status.lock().unwrap();
                                    st.last_error = Some(format!("ruajtja e rezultatit dështoi: {e}"));
                                } else {
                                    let mut st = status.lock().unwrap();
                                    st.last_message_at = Some(crate::util::now_iso());
                                    st.last_error = None;
                                }
                            }
                            acc.clear();
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                        // Normale kur analizuesi s'ka dërguar asgjë ende - vazhdo.
                    }
                    Err(e) => {
                        let mut st = status.lock().unwrap();
                        st.last_error = Some(format!("gabim leximi porti serial: {e}"));
                        std::thread::sleep(Duration::from_millis(1000));
                    }
                }
            }
        });

        *self.thread.lock().unwrap() = Some(handle);
        Ok(())
    }
}

#[cfg(not(desktop))]
impl LabSerialManager {
    pub fn connect(
        &self,
        _db: Arc<Db>,
        _port_name: String,
        _profile: AnalyzerProfile,
    ) -> anyhow::Result<()> {
        anyhow::bail!("Lidhja me analizues laboratorik mbështetet vetëm në desktop (Windows/macOS).")
    }
}

impl Default for LabSerialManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(desktop)]
pub fn list_serial_ports() -> Vec<String> {
    serialport::available_ports()
        .map(|ports| ports.into_iter().map(|p| p.port_name).collect())
        .unwrap_or_default()
}

#[cfg(not(desktop))]
pub fn list_serial_ports() -> Vec<String> {
    Vec::new()
}
