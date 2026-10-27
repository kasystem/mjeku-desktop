-- Expanded official ATK catalog from PROGRAMERI POS, additive and idempotent.
-- Keep the legacy v4 table/triggers for existing receipts; new selections and
-- their immutable snapshots live in this table.
CREATE TABLE IF NOT EXISTS sef_mjeku_item_catalog_v7 (
 visit_item_id TEXT PRIMARY KEY REFERENCES visit_items(id),
 category_code TEXT NOT NULL CHECK(category_code IN ('TT','UR','AU','VM','KO','AP','BM','PM','PP','PE','PS','PB','BB','KB','EE','PSE','SM','SJ','ST','SK','SMK','STR','PI','PT','SEA','KS','MN','SHN','SDG','HT','SHB','STI','SHT','PBS','MPM','PPD','PPH','PPP','PPA','PHN','SHH','SUA','SHZ','PJA','PAL2','DUH','SHM','PRO','SFV','BSH','RPK','PTI')),
 unit_code TEXT NOT NULL CHECK(unit_code IN ('EA','CP','LTR','SEC','MIN','HUR','DAY','MON','ANN','KGB','KGN','TN','GRM','M4','UDH','KMT','LM','MTS','MIV','MSK','KEH','NFJ','NFQ','PRQ','DP','OP','PRD','PAL','XBX','PK','SET')),
 created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);
INSERT OR IGNORE INTO sef_mjeku_item_catalog_v7
 (visit_item_id,category_code,unit_code,created_at,updated_at)
SELECT visit_item_id,category_code,unit_code,created_at,updated_at
FROM sef_mjeku_item_catalog;
CREATE TRIGGER IF NOT EXISTS sef_mjeku_catalog_v7_no_delete
 BEFORE DELETE ON sef_mjeku_item_catalog_v7
 BEGIN SELECT RAISE(ABORT,'SEF catalog cannot be deleted'); END;
CREATE TRIGGER IF NOT EXISTS sef_mjeku_catalog_v7_no_update_after_request
 BEFORE UPDATE ON sef_mjeku_item_catalog_v7
 WHEN EXISTS(SELECT 1 FROM visit_items i
   JOIN sef_mjeku_transactions s ON s.sale_id=i.visit_id
   WHERE i.id=OLD.visit_item_id)
 BEGIN SELECT RAISE(ABORT,'SEF catalog frozen after fiscal request'); END;
CREATE TRIGGER IF NOT EXISTS sef_mjeku_catalog_v7_no_insert_after_request
 BEFORE INSERT ON sef_mjeku_item_catalog_v7
 WHEN EXISTS(SELECT 1 FROM visit_items i
   JOIN sef_mjeku_transactions s ON s.sale_id=i.visit_id
   WHERE i.id=NEW.visit_item_id)
 BEGIN SELECT RAISE(ABORT,'SEF catalog frozen after fiscal request'); END;
CREATE TRIGGER IF NOT EXISTS sef_mjeku_catalog_v7_no_update_after_fiscalization
 BEFORE UPDATE ON sef_mjeku_item_catalog_v7
 WHEN EXISTS(SELECT 1 FROM visit_items
   WHERE id=OLD.visit_item_id AND fiscalized=1)
 BEGIN SELECT RAISE(ABORT,'SEF item frozen after fiscalization'); END;
CREATE TRIGGER IF NOT EXISTS sef_mjeku_catalog_v7_no_insert_after_fiscalization
 BEFORE INSERT ON sef_mjeku_item_catalog_v7
 WHEN EXISTS(SELECT 1 FROM visit_items
   WHERE id=NEW.visit_item_id AND fiscalized=1)
 BEGIN SELECT RAISE(ABORT,'SEF item frozen after fiscalization'); END;
