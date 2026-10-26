-- Explicit, non-clinical fiscal classification per billable visit item.
-- Required by versioned SEF v2 payload; nothing is automatically inferred from a diagnosis.
CREATE TABLE IF NOT EXISTS sef_mjeku_item_catalog (
 visit_item_id TEXT PRIMARY KEY REFERENCES visit_items(id),
 category_code TEXT NOT NULL CHECK(category_code IN ('SM','BM','PM')),
 unit_code TEXT NOT NULL CHECK(unit_code IN ('EA','HUR','DAY')),
 created_at TEXT NOT NULL,
 updated_at TEXT NOT NULL
);
CREATE TRIGGER IF NOT EXISTS sef_mjeku_catalog_no_delete
 BEFORE DELETE ON sef_mjeku_item_catalog
 BEGIN SELECT RAISE(ABORT,'SEF item catalog cannot be deleted'); END;
CREATE TRIGGER IF NOT EXISTS sef_mjeku_catalog_no_update_after_fiscalization
 BEFORE UPDATE ON sef_mjeku_item_catalog
 WHEN EXISTS(SELECT 1 FROM visit_items WHERE id=OLD.visit_item_id AND fiscalized=1)
 BEGIN SELECT RAISE(ABORT,'SEF catalog frozen after fiscalization'); END;
CREATE TRIGGER IF NOT EXISTS sef_mjeku_catalog_no_update_after_request
 BEFORE UPDATE ON sef_mjeku_item_catalog
 WHEN EXISTS(
  SELECT 1 FROM visit_items i JOIN sef_mjeku_transactions s ON s.sale_id=i.visit_id
  WHERE i.id=OLD.visit_item_id
 )
 BEGIN SELECT RAISE(ABORT,'SEF catalog frozen after first fiscal request'); END;
CREATE TRIGGER IF NOT EXISTS sef_mjeku_catalog_no_insert_after_request
 BEFORE INSERT ON sef_mjeku_item_catalog
 WHEN EXISTS(
  SELECT 1 FROM visit_items i JOIN sef_mjeku_transactions s ON s.sale_id=i.visit_id
  WHERE i.id=NEW.visit_item_id
 )
 BEGIN SELECT RAISE(ABORT,'SEF catalog cannot be added after first fiscal request'); END;
