use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use anyhow::Context;

use crate::util::now_iso;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ErrorLogEntry {
    pub timestamp: String,
    pub source: String,
    pub message: String,
}

fn sanitize_line(s: &str) -> String {
    s.replace('\r', " ").replace('\n', " ").trim().to_string()
}

pub fn append(path: &Path, source: &str, message: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create log dir: {}", parent.display()))?;
    }
    let ts = now_iso();
    let src = sanitize_line(source);
    let msg = sanitize_line(message);
    let line = format!("{ts}\t{src}\t{msg}\n");
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("open log file: {}", path.display()))?;
    f.write_all(line.as_bytes())
        .with_context(|| format!("write log file: {}", path.display()))?;
    let _ = f.flush();
    Ok(())
}

pub fn list(path: &Path, limit: usize) -> anyhow::Result<Vec<ErrorLogEntry>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let f = OpenOptions::new()
        .read(true)
        .open(path)
        .with_context(|| format!("read log file: {}", path.display()))?;
    let r = BufReader::new(f);
    let mut rows: Vec<ErrorLogEntry> = Vec::new();
    for line in r.lines() {
        let line = line.unwrap_or_default();
        if line.trim().is_empty() {
            continue;
        }
        let mut parts = line.splitn(3, '\t');
        let timestamp = parts.next().unwrap_or("").to_string();
        let source = parts.next().unwrap_or("").to_string();
        let message = parts.next().unwrap_or("").to_string();
        rows.push(ErrorLogEntry {
            timestamp,
            source,
            message,
        });
    }
    // Newest first.
    rows.reverse();
    if rows.len() > limit {
        rows.truncate(limit);
    }
    Ok(rows)
}

pub fn clear(path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create log dir: {}", parent.display()))?;
    }
    fs::write(path, b"").with_context(|| format!("clear log file: {}", path.display()))?;
    Ok(())
}
