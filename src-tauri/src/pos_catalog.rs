//! Mirrors the clinic catalog (services, offers) into PROGRAMERI POS via the local fiscal bridge
//! (`catalog.sync`). POS shows services as stockless "Shërbim" products and offers under Ofertat.
//! Two-way for services (`catalog.pending` / `catalog.ack`): a service edited or created in POS is applied
//! here (the newer edit wins) and then synced to the cloud like any local edit.

use crate::db::Db;
use sha2::{Digest, Sha256};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const KEY_HASH: &str = "pos_catalog_hash";
const KEY_PUSHED_AT: &str = "pos_catalog_pushed_at";
/// Re-send once a day even when unchanged (POS restored from backup, products edited there, …).
const REFRESH_SECS: i64 = 24 * 60 * 60;

fn now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn get(db: &Db, key: &str) -> String {
    db.setting_get(key).ok().flatten().unwrap_or_default()
}

pub fn content_hash(params: &serde_json::Value) -> String {
    hex::encode(Sha256::digest(params.to_string().as_bytes()))
}

/// One service change made in PROGRAMERI POS (`catalog.pending`).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PosServiceChange {
    pub local_ref: i64,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub price_cents: i64,
    #[serde(default)]
    pub vat_code: String,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub atk_category_code: String,
    #[serde(default)]
    pub atk_unit_code: String,
}

/// ATK code from POS when it is a known official code; None keeps the service's current code.
fn atk_from_pos(code: &str, valid: fn(&str) -> bool) -> Option<String> {
    let c = code.trim().to_uppercase();
    if !c.is_empty() && valid(&c) {
        Some(c)
    } else {
        None
    }
}

fn is_newer(a: &str, b: &str) -> bool {
    match (crate::util::parse_rfc3339_to_utc(a), crate::util::parse_rfc3339_to_utc(b)) {
        (Ok(x), Ok(y)) => x > y,
        // POS time unknown: apply (the edit was made after the last sync, otherwise it would not be pending).
        (Err(_), _) => true,
        (Ok(_), Err(_)) => true,
    }
}

/// Applies one POS change. Returns the Mjeku service id to acknowledge, or None when the local service
/// was edited more recently (Mjeku's content is pushed next and wins in POS).
pub fn apply_pos_change(db: &Db, c: &PosServiceChange) -> anyhow::Result<Option<String>> {
    let id = c.id.trim().to_string();
    let existing = if id.is_empty() { None } else { db.service_notes_and_vat(&id)? };
    if existing.is_some() {
        if let Some(local) = db.services_updated_at(&id)? {
            if !is_newer(&c.updated_at, &local) {
                return Ok(None);
            }
        }
    }
    if c.deleted {
        if existing.is_some() {
            db.services_delete(&id)?;
        }
        return Ok(Some(if id.is_empty() { uuid::Uuid::new_v4().to_string() } else { id }));
    }
    let title = c.title.trim();
    if title.is_empty() || c.price_cents < 0 {
        anyhow::bail!("ndryshim i pavlefshëm nga POS");
    }
    let code = c.vat_code.trim().to_uppercase();
    let vat_code = if matches!(code.as_str(), "A" | "C" | "D" | "E") {
        Some(code)
    } else {
        existing.as_ref().map(|e| e.1.clone())
    };
    let row = db.services_upsert(crate::models::ServiceUpsertInput {
        id: if id.is_empty() { None } else { Some(id) },
        title: title.to_string(),
        default_price: c.price_cents as f64 / 100.0,
        vat_code,
        atk_category_code: atk_from_pos(&c.atk_category_code, crate::atk_catalog::valid_category_code),
        atk_unit_code: atk_from_pos(&c.atk_unit_code, crate::atk_catalog::valid_unit_code),
        sku: None,
        barcode: None,
        category: None,
        subcategory: None,
        notes: existing.and_then(|e| e.0),
    })?;
    Ok(Some(row.id))
}

/// Pulls services edited in POS, applies them and acknowledges. An older POS without
/// `catalog.pending` answers with an error, which is ignored.
pub async fn pull_pos_changes(db: &Db, clinic_id: &str) {
    let Ok(v) = crate::sef_bridge::call("catalog.pending", serde_json::json!({ "clinicId": clinic_id }), Duration::from_secs(30)).await else {
        return;
    };
    let changes: Vec<PosServiceChange> = v
        .get("services")
        .cloned()
        .and_then(|x| serde_json::from_value(x).ok())
        .unwrap_or_default();
    if changes.is_empty() {
        return;
    }
    let mut links = Vec::new();
    for c in &changes {
        match apply_pos_change(db, c) {
            Ok(Some(id)) => links.push(serde_json::json!({ "localRef": c.local_ref, "id": id })),
            Ok(None) => {}
            Err(e) => eprintln!("[pos_catalog] POS change {} not applied: {e}", c.local_ref),
        }
    }
    if !links.is_empty() {
        let _ = crate::sef_bridge::call("catalog.ack", serde_json::json!({ "clinicId": clinic_id, "links": links }), Duration::from_secs(30)).await;
    }
}

/// Never fails the caller: an older POS without `catalog.sync` simply answers with an error.
pub async fn push_if_changed(db: &Db) {
    if !crate::sef_bridge::launched_from_programeri() {
        return;
    }
    let clinic_id = get(db, "clinic_id");
    if clinic_id.trim().is_empty() {
        return;
    }
    pull_pos_changes(db, &clinic_id).await;
    let Ok((services, offers)) = db.pos_catalog_snapshot() else {
        return;
    };
    let params = serde_json::json!({ "clinicId": clinic_id, "services": services, "offers": offers });
    let hash = content_hash(&params);
    let pushed_at: i64 = get(db, KEY_PUSHED_AT).parse().unwrap_or(0);
    if get(db, KEY_HASH) == hash && now_secs() - pushed_at < REFRESH_SECS {
        return;
    }
    if crate::sef_bridge::call("catalog.sync", params, Duration::from_secs(60)).await.is_ok() {
        let _ = db.setting_set(KEY_HASH, &hash);
        let _ = db.setting_set(KEY_PUSHED_AT, &now_secs().to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(id: &str, title: &str, cents: i64, deleted: bool, at: &str) -> PosServiceChange {
        PosServiceChange { local_ref: 7, id: id.into(), title: title.into(), price_cents: cents, vat_code: "C".into(), deleted, updated_at: at.into(), atk_category_code: String::new(), atk_unit_code: String::new() }
    }

    #[test]
    fn pos_edits_apply_newer_wins_and_create_new_services() {
        let db = Db::new_in_memory().unwrap();
        db.execute_batch_for_tests(
            "INSERT INTO services(id,title,default_price,vat_code,notes,created_at,updated_at,deleted) VALUES('s1','Mbushje',25,'C','shënim','2026-11-01T00:00:00Z','2026-11-05T00:00:00Z',0);",
        )
        .unwrap();
        // Newer POS edit: applied, notes kept.
        assert_eq!(apply_pos_change(&db, &change("s1", "Mbushje e re", 4000, false, "2026-11-06T00:00:00Z")).unwrap().as_deref(), Some("s1"));
        let s = db.services_list(None).unwrap().into_iter().find(|x| x.id == "s1").unwrap();
        assert_eq!(s.title, "Mbushje e re");
        assert_eq!(s.default_price, 40.0);
        assert_eq!(s.notes.as_deref(), Some("shënim"));
        // Older POS edit than the local one: skipped (no ack).
        assert!(apply_pos_change(&db, &change("s1", "E vjetër", 100, false, "2000-01-01T00:00:00Z")).unwrap().is_none());
        // Created in POS: new Mjeku service with a new id.
        let new_id = apply_pos_change(&db, &change("", "Zbardhim", 5000, false, "")).unwrap().unwrap();
        assert!(db.services_list(None).unwrap().iter().any(|x| x.id == new_id && x.title == "Zbardhim"));
        // Deleted in POS.
        assert!(apply_pos_change(&db, &change(&new_id, "", 0, true, "2100-01-01T00:00:00Z")).unwrap().is_some());
        assert!(!db.services_list(None).unwrap().iter().any(|x| x.id == new_id));

        // ATK codes from POS: a known code is applied, an unknown or empty one keeps the current code.
        let mut atk = change("s1", "Mbushje e re", 4000, false, "2100-01-02T00:00:00Z");
        atk.atk_unit_code = "SEC".to_string();
        atk.atk_category_code = "XX".to_string();
        apply_pos_change(&db, &atk).unwrap();
        let s = db.services_list(None).unwrap().into_iter().find(|x| x.id == "s1").unwrap();
        assert_eq!(s.atk_unit_code, "SEC");
        assert_eq!(s.atk_category_code, "SM");
    }

    #[test]
    fn hash_changes_only_with_content() {
        let a = serde_json::json!({ "clinicId": "c", "services": [{ "id": "1", "priceCents": 2500 }], "offers": [] });
        let b = serde_json::json!({ "clinicId": "c", "services": [{ "id": "1", "priceCents": 2500 }], "offers": [] });
        let c = serde_json::json!({ "clinicId": "c", "services": [{ "id": "1", "priceCents": 3000 }], "offers": [] });
        assert_eq!(content_hash(&a), content_hash(&b));
        assert_ne!(content_hash(&a), content_hash(&c));
    }

    #[test]
    fn snapshot_sends_services_and_gross_offer_prices() {
        let db = Db::new_in_memory().unwrap();
        db.execute_batch_for_tests(
            "INSERT INTO services(id,title,default_price,vat_code,deleted) VALUES('s1','Mbushje',25.5,'C',0),('s2','Vjetër',10,'C',1);
             INSERT INTO clients(id,name) VALUES('c1','Arben');
             INSERT INTO offers(id,client_id,offer_number,status,vat_pct,created_at) VALUES('o1','c1','OF-1','sent',18,'2026-11-07');
             INSERT INTO offer_items(id,offer_id,description,qty,unit_price,discount_pct) VALUES('i1','o1','Implant',2,100,0);",
        )
        .unwrap();
        let (services, offers) = db.pos_catalog_snapshot().unwrap();
        assert_eq!(services.len(), 2);
        assert_eq!(services[0]["priceCents"], 2550);
        assert_eq!(services[1]["deleted"], true);
        assert_eq!(services[0]["atkCategoryCode"], "SM");
        assert_eq!(services[0]["atkUnitCode"], "CP");
        assert_eq!(offers[0]["clientName"], "Arben");
        assert_eq!(offers[0]["items"][0]["unitPriceCents"], 11800);
    }
}
