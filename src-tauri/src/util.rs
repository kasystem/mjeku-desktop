use anyhow::Context;
use chrono::{DateTime, SecondsFormat, Utc};
use std::error::Error as StdError;

pub fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub fn parse_rfc3339_to_utc(s: &str) -> anyhow::Result<DateTime<Utc>> {
    let dt = DateTime::parse_from_rfc3339(s).with_context(|| format!("invalid timestamp: {s}"))?;
    Ok(dt.with_timezone(&Utc))
}

pub fn is_network_error(err: &reqwest::Error) -> bool {
    err.is_timeout() || err.is_connect() || err.is_request() || err.is_body() || err.is_decode()
}

pub fn reqwest_error_detailed(err: &reqwest::Error) -> String {
    let mut parts: Vec<String> = vec![err.to_string()];
    let mut src = err.source();
    while let Some(s) = src {
        parts.push(s.to_string());
        src = s.source();
    }
    let mut out = parts.join(" | ");
    if let Some(url) = err.url() {
        out.push_str(&format!(" [url: {}]", url));
    }
    out
}
