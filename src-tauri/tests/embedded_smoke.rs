#![cfg(feature = "embedded")]
//! Prova e Mjekut "embedded" (brenda PROGRAMERI POS Mobile): niset pa Tauri në një dosje të përkohshme
//! dhe thërret komandat me të njëjtën kontratë JSON që përdor UI-ja.
use std::ffi::{CStr, CString};

use mjeku_desktop_lib::embedded::*;

fn call(cmd: &str, args: &str) -> serde_json::Value {
    let c = CString::new(cmd).unwrap();
    let a = CString::new(args).unwrap();
    let p = mjeku_embedded_invoke(c.as_ptr(), a.as_ptr());
    let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    mjeku_embedded_free(p);
    serde_json::from_str(&s).unwrap()
}

#[test]
fn starts_and_answers_commands() {
    let dir = std::env::temp_dir().join(format!("mjeku-embedded-smoke-{}", std::process::id()));
    let d = CString::new(dir.display().to_string()).unwrap();
    let v = CString::new("0.6.4-embedded").unwrap();
    let p = mjeku_embedded_start(d.as_ptr(), v.as_ptr());
    let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    mjeku_embedded_free(p);
    let started: serde_json::Value = serde_json::from_str(&s).unwrap();
    assert_eq!(started["ok"], true, "start: {s}");
    let ui = started["ui_dir"].as_str().expect("ui_dir");
    assert!(std::path::Path::new(ui).join("index.html").exists(), "UI seed u nxor: {ui}");

    let info = call("get_app_info", "{}");
    assert_eq!(info["ok"], true, "{info}");
    assert_eq!(info["value"]["version"], "0.6.4-embedded", "{info}");

    let auth = call("auth_get_state", "");
    assert_eq!(auth["ok"], true, "{auth}");

    // Pa login: komandat e mbrojtura refuzohen (jo panik).
    let denied = call("clients_list", r#"{"search":null}"#);
    assert_eq!(denied["ok"], false, "{denied}");

    let unknown = call("nuk_ekziston", "{}");
    assert_eq!(unknown["ok"], false);
    println!("auth={auth}\ndenied={denied}");
    let _ = std::fs::remove_dir_all(&dir);
}
