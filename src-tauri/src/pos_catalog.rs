//! Mirrors the clinic catalog (services, offers) into PROGRAMERI POS via the local fiscal bridge
//! (`catalog.sync`). POS shows services as stockless "Shërbim" products and offers under Ofertat.
//! Mjeku stays the master; the push happens only when launched by POS and only when the content changed.

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

/// Never fails the caller: an older POS without `catalog.sync` simply answers with an error.
pub async fn push_if_changed(db: &Db) {
    if !crate::sef_bridge::launched_from_programeri() {
        return;
    }
    let clinic_id = get(db, "clinic_id");
    if clinic_id.trim().is_empty() {
        return;
    }
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
        assert_eq!(offers[0]["clientName"], "Arben");
        assert_eq!(offers[0]["items"][0]["unitPriceCents"], 11800);
    }
}
