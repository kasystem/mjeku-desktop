//! Later payments of SEF sales → PROGRAMERI POS accounting (`payments.sync`, capability `mjeku_payments_v1`).
//! For every sale with payments Mjeku sends what it has collected so far, split into cash and bank
//! (card/bank). POS compares it with what is already posted for that sale's document and posts only the
//! difference against receivables, so a debt paid later reaches the accounting books exactly once.
//! Payments that were deleted count as zero, so a correction is reversed in POS too.

use crate::db::Db;
use std::time::Duration;

const KEY_HASH: &str = "pos_payments_hash";

/// Totals per sale: `{ saleId, cashCents, bankCents, lastPaidAt }`.
pub fn snapshot(db: &Db) -> anyhow::Result<Vec<serde_json::Value>> {
    db.pos_payment_totals()
}

pub async fn push_if_changed(db: &Db) {
    if !crate::sef_bridge::launched_from_programeri() {
        return;
    }
    let Ok(sales) = snapshot(db) else {
        return;
    };
    if sales.is_empty() {
        return;
    }
    let params = serde_json::json!({ "sales": sales });
    let hash = crate::pos_catalog::content_hash(&params);
    if db.setting_get(KEY_HASH).ok().flatten().unwrap_or_default() == hash {
        return;
    }
    // An older PROGRAMERI POS without payments.sync answers with an error: try again on the next tick.
    if crate::sef_bridge::call("payments.sync", params, Duration::from_secs(120)).await.is_ok() {
        let _ = db.setting_set(KEY_HASH, &hash);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn totals_split_cash_and_bank_and_deleted_payments_count_as_zero() {
        let db = Db::new_in_memory().unwrap();
        db.execute_batch_for_tests(
            "INSERT INTO payments(id,client_id,sale_id,date,amount,method,created_at,updated_at,deleted) VALUES
             ('p1','c1','s1','2026-11-01',20,'cash','2026-11-01','2026-11-01',0),
             ('p2','c1','s1','2026-11-05',30.5,'card','2026-11-05','2026-11-05',0),
             ('p3','c1','s1','2026-11-06',10,'bank','2026-11-06','2026-11-06',0),
             ('p4','c1','s2','2026-11-02',15,'cash','2026-11-02','2026-11-02',1),
             ('p5','c1',NULL,'2026-11-02',99,'cash','2026-11-02','2026-11-02',0);",
        )
        .unwrap();
        let rows = snapshot(&db).unwrap();
        let s1 = rows.iter().find(|r| r["saleId"] == "s1").unwrap();
        assert_eq!(s1["cashCents"], 2000);
        assert_eq!(s1["bankCents"], 4050);
        assert_eq!(s1["lastPaidAt"], "2026-11-06");
        let s2 = rows.iter().find(|r| r["saleId"] == "s2").unwrap();
        assert_eq!(s2["cashCents"], 0);
        assert_eq!(rows.len(), 2);
    }
}
