//! Clinic data → PROGRAMERI POS (`clinic.sync`, capability `mjeku_clinic_data_v1`):
//! - patients become POS clients with identity/contact only (name, phone, city) — never clinical data;
//! - stock items become POS stock products, and every Mjeku stock movement becomes one POS stock adjustment;
//! - POS answers with its stock per item (purchases made in POS included) and Mjeku records the difference as a
//!   `pos-sync:` movement, which is never sent back.

use crate::db::Db;
use std::time::Duration;

const KEY_HASH: &str = "pos_clinic_hash";
pub const POS_SYNC_PREFIX: &str = "pos-sync:";

pub fn snapshot(db: &Db, clinic_id: &str) -> anyhow::Result<serde_json::Value> {
    let (patients, items, movements) = db.pos_clinic_snapshot(POS_SYNC_PREFIX)?;
    Ok(serde_json::json!({
        "clinicId": clinic_id,
        "patients": patients,
        "stockItems": items,
        "stockMovements": movements,
    }))
}

/// Records POS stock for each mirrored item as a local movement so both sides show the same quantity.
pub fn apply_stock_levels(db: &Db, levels: &[serde_json::Value]) -> anyhow::Result<usize> {
    let mut applied = 0;
    for l in levels {
        let item_id = l.get("itemId").and_then(|v| v.as_str()).unwrap_or("").trim();
        let Some(pos_qty) = l.get("quantity").and_then(|v| v.as_f64()) else { continue };
        if item_id.is_empty() || !pos_qty.is_finite() {
            continue;
        }
        let Some(local) = db.stock_item_quantity_for_sync(item_id)? else { continue };
        let diff = ((pos_qty - local) * 10000.0).round() / 10000.0;
        if diff.abs() < 0.0001 {
            continue;
        }
        let now = crate::util::now_iso();
        let m = crate::models::StockMovement {
            deleted: 0,
            id: format!("{POS_SYNC_PREFIX}{}", uuid::Uuid::new_v4()),
            item_id: item_id.to_string(),
            movement_type: if diff > 0.0 { "adjustment".to_string() } else { "out".to_string() },
            quantity: diff.abs(),
            price_per_unit: None,
            notes: Some("Sinkronizim me PROGRAMERI POS".to_string()),
            date: now.chars().take(10).collect(),
            created_at: now.clone(),
            updated_at: now,
        };
        db.stock_movement_add(&m)?;
        applied += 1;
    }
    Ok(applied)
}

pub async fn push_if_changed(db: &Db) {
    if !crate::sef_bridge::launched_from_programeri() {
        return;
    }
    let clinic_id = db.setting_get("clinic_id").ok().flatten().unwrap_or_default();
    if clinic_id.trim().is_empty() {
        return;
    }
    let Ok(params) = snapshot(db, clinic_id.trim()) else {
        return;
    };
    let hash = crate::pos_catalog::content_hash(&params);
    if db.setting_get(KEY_HASH).ok().flatten().unwrap_or_default() == hash {
        return;
    }
    // An older PROGRAMERI POS without clinic.sync answers with an error: try again on the next tick.
    let Ok(result) = crate::sef_bridge::call("clinic.sync", params, Duration::from_secs(180)).await else {
        return;
    };
    let levels = result.get("stockLevels").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    match apply_stock_levels(db, &levels) {
        // New pos-sync movements change nothing that is sent (they are filtered), so the hash stays valid.
        Ok(_) => {
            let _ = db.setting_set(KEY_HASH, &hash);
        }
        Err(e) => eprintln!("[pos_clinic] stock levels not applied: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_sends_contacts_only_and_skips_pos_sync_movements() {
        let db = Db::new_in_memory().unwrap();
        db.execute_batch_for_tests(
            "INSERT INTO clients(id,name,phone,city,allergies,dob,created_at,updated_at,deleted) VALUES('c1','Arben K','044','Prizren','penicilina','1990-01-01','x','x',0);
             INSERT INTO stock_items(id,name,unit,created_at,updated_at,deleted) VALUES('s1','Anestetik','amp','x','x',0);
             INSERT INTO stock_movements(id,item_id,movement_type,quantity,price_per_unit,date,created_at) VALUES('m1','s1','in',10,2.5,'2026-11-01','x'),('pos-sync:1','s1','adjustment',5,NULL,'2026-11-02','x');",
        )
        .unwrap();
        let v = snapshot(&db, "c").unwrap();
        let p = &v["patients"][0];
        assert_eq!(p["name"], "Arben K");
        assert_eq!(p["city"], "Prizren");
        assert!(p.get("allergies").is_none());
        assert!(p.get("dob").is_none());
        assert_eq!(v["stockItems"][0]["unit"], "amp");
        assert_eq!(v["stockMovements"].as_array().unwrap().len(), 1);
        assert_eq!(v["stockMovements"][0]["id"], "m1");
    }

    #[test]
    fn pos_stock_levels_become_local_movements() {
        let db = Db::new_in_memory().unwrap();
        db.execute_batch_for_tests(
            "INSERT INTO stock_items(id,name,unit,created_at,updated_at,deleted) VALUES('s1','Anestetik','amp','x','x',0);
             INSERT INTO stock_movements(id,item_id,movement_type,quantity,date,created_at) VALUES('m1','s1','in',10,'2026-11-01','x');",
        )
        .unwrap();
        // POS has 15 (a purchase of 5 made in POS).
        assert_eq!(apply_stock_levels(&db, &[serde_json::json!({ "itemId": "s1", "quantity": 15 })]).unwrap(), 1);
        assert_eq!(db.stock_item_quantity_for_sync("s1").unwrap(), Some(15.0));
        // Same level again: nothing to do. Lower level: an "out" movement.
        assert_eq!(apply_stock_levels(&db, &[serde_json::json!({ "itemId": "s1", "quantity": 15 })]).unwrap(), 0);
        apply_stock_levels(&db, &[serde_json::json!({ "itemId": "s1", "quantity": 12 })]).unwrap();
        assert_eq!(db.stock_item_quantity_for_sync("s1").unwrap(), Some(12.0));
    }
}
