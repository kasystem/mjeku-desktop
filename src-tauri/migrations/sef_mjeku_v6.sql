-- Fiscal-only metadata for ordinary SalesPage items (NOT clinical visit_items).
-- No default tax rate, no inferred clinical description, no implicit cash payment.
CREATE TABLE IF NOT EXISTS sef_mjeku_sale_item_fiscal (
  sale_item_id TEXT PRIMARY KEY REFERENCES sale_items(id) ON DELETE CASCADE,
  fiscal_title TEXT NOT NULL CHECK(length(trim(fiscal_title)) BETWEEN 3 AND 120),
  tax_code TEXT NOT NULL CHECK(tax_code IN ('A','C','D','E')),
  category_code TEXT NOT NULL CHECK(category_code IN ('TT','UR','AU','VM','KO','AP','BM','PM','PP','PE','PS','PB','BB','KB','EE','PSE','SM','SJ','ST','SK','SMK','STR','PI','PT','SEA','KS','MN','SHN','SDG','HT','SHB','STI','SHT','PBS','MPM','PPD','PPH','PPP','PPA','PHN','SHH','SUA','SHZ','PJA','PAL2','DUH','SHM','PRO','SFV','BSH','RPK','PTI')),
  unit_code TEXT NOT NULL CHECK(unit_code IN ('EA','CP','LTR','SEC','MIN','HUR','DAY','MON','ANN','KGB','KGN','TN','GRM','M4','UDH','KMT','LM','MTS','MIV','MSK','KEH','NFJ','NFQ','PRQ','DP','OP','PRD','PAL','XBX','PK','SET')),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE TRIGGER IF NOT EXISTS sef_mjeku_sale_item_fiscal_no_change_after_request
BEFORE UPDATE ON sef_mjeku_sale_item_fiscal
WHEN EXISTS(
  SELECT 1 FROM sale_items i JOIN sef_mjeku_transactions t ON t.sale_id=i.sale_id
  WHERE i.id=OLD.sale_item_id
)
BEGIN SELECT RAISE(ABORT,'SEF item frozen after first fiscal request'); END;
CREATE TRIGGER IF NOT EXISTS sef_mjeku_sale_item_fiscal_no_delete_after_request
BEFORE DELETE ON sef_mjeku_sale_item_fiscal
WHEN EXISTS(
  SELECT 1 FROM sale_items i JOIN sef_mjeku_transactions t ON t.sale_id=i.sale_id
  WHERE i.id=OLD.sale_item_id
)
BEGIN SELECT RAISE(ABORT,'SEF item frozen after first fiscal request'); END;
CREATE TRIGGER IF NOT EXISTS sef_mjeku_sale_item_fiscal_no_insert_after_request
BEFORE INSERT ON sef_mjeku_sale_item_fiscal
WHEN EXISTS(
  SELECT 1 FROM sale_items i JOIN sef_mjeku_transactions t ON t.sale_id=i.sale_id
  WHERE i.id=NEW.sale_item_id
)
BEGIN SELECT RAISE(ABORT,'SEF item frozen after first fiscal request'); END;

-- One-time ATK catalog assignment to a service/product, NOT per invoice.
-- Services use their immutable catalog tax code; products use configured profile.
CREATE TABLE IF NOT EXISTS sef_mjeku_catalog_templates (
  item_type TEXT NOT NULL CHECK(item_type IN ('service','product')),
  ref_id TEXT NOT NULL,
  tax_code TEXT NOT NULL DEFAULT '' CHECK(tax_code IN ('','A','C','D','E')),
  category_code TEXT NOT NULL CHECK(category_code IN ('TT','UR','AU','VM','KO','AP','BM','PM','PP','PE','PS','PB','BB','KB','EE','PSE','SM','SJ','ST','SK','SMK','STR','PI','PT','SEA','KS','MN','SHN','SDG','HT','SHB','STI','SHT','PBS','MPM','PPD','PPH','PPP','PPA','PHN','SHH','SUA','SHZ','PJA','PAL2','DUH','SHM','PRO','SFV','BSH','RPK','PTI')),
  unit_code TEXT NOT NULL CHECK(unit_code IN ('EA','CP','LTR','SEC','MIN','HUR','DAY','MON','ANN','KGB','KGN','TN','GRM','M4','UDH','KMT','LM','MTS','MIV','MSK','KEH','NFJ','NFQ','PRQ','DP','OP','PRD','PAL','XBX','PK','SET')),
  updated_at TEXT NOT NULL,
  PRIMARY KEY(item_type, ref_id)
);
