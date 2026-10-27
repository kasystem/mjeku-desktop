-- Fiscal-only metadata for ordinary SalesPage items (NOT clinical visit_items).
-- No default tax rate, no inferred clinical description, no implicit cash payment.
CREATE TABLE IF NOT EXISTS sef_mjeku_sale_item_fiscal (
  sale_item_id TEXT PRIMARY KEY REFERENCES sale_items(id) ON DELETE CASCADE,
  fiscal_title TEXT NOT NULL CHECK(length(trim(fiscal_title)) BETWEEN 3 AND 120),
  tax_code TEXT NOT NULL CHECK(tax_code IN ('A','C','D','E')),
  category_code TEXT NOT NULL CHECK(category_code IN ('SM','BM','PM')),
  unit_code TEXT NOT NULL CHECK(unit_code IN ('EA','HUR','DAY')),
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
