-- Additive Mjeku product/service ATK catalog and stock metadata.
-- Apply to Supabase before enabling sync for these fields. Existing rows are preserved;
-- old stock items remain `unknown` and unclassified until an operator reviews them.

ALTER TABLE public.services
  ADD COLUMN IF NOT EXISTS atk_category_code text NOT NULL DEFAULT 'SM',
  ADD COLUMN IF NOT EXISTS atk_unit_code text NOT NULL DEFAULT 'CP',
  ADD COLUMN IF NOT EXISTS sku text,
  ADD COLUMN IF NOT EXISTS barcode text,
  ADD COLUMN IF NOT EXISTS category text,
  ADD COLUMN IF NOT EXISTS subcategory text;

ALTER TABLE public.visit_items
  ADD COLUMN IF NOT EXISTS catalog_item_id uuid,
  ADD COLUMN IF NOT EXISTS catalog_item_type text,
  ADD COLUMN IF NOT EXISTS atk_category_code text NOT NULL DEFAULT '',
  ADD COLUMN IF NOT EXISTS atk_unit_code text NOT NULL DEFAULT '';
CREATE INDEX IF NOT EXISTS visit_items_catalog_item_idx
  ON public.visit_items (catalog_item_type, catalog_item_id);

CREATE INDEX IF NOT EXISTS services_sku_idx
  ON public.services (lower(trim(sku)))
  WHERE sku IS NOT NULL AND trim(sku) <> '';
CREATE INDEX IF NOT EXISTS services_barcode_idx
  ON public.services (trim(barcode))
  WHERE barcode IS NOT NULL AND trim(barcode) <> '';

CREATE TABLE IF NOT EXISTS public.stock_suppliers (
  id uuid PRIMARY KEY,
  clinic_id uuid,
  name text NOT NULL,
  phone text,
  email text,
  address text,
  notes text,
  created_at timestamptz NOT NULL,
  updated_at timestamptz NOT NULL,
  deleted integer NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS public.stock_items (
  id uuid PRIMARY KEY,
  clinic_id uuid,
  name text NOT NULL,
  unit text,
  category text,
  supplier_id uuid,
  min_quantity double precision NOT NULL DEFAULT 0,
  sale_price double precision NOT NULL DEFAULT 0,
  purchase_price double precision NOT NULL DEFAULT 0,
  stock_kind text NOT NULL DEFAULT 'unknown',
  atk_tax_code text NOT NULL DEFAULT 'C',
  atk_category_code text NOT NULL DEFAULT '',
  atk_unit_code text NOT NULL DEFAULT '',
  sku text,
  barcode text,
  subcategory text,
  purchase_unit text,
  purchase_unit_multiplier double precision NOT NULL DEFAULT 1,
  notes text,
  created_at timestamptz NOT NULL,
  updated_at timestamptz NOT NULL,
  deleted integer NOT NULL DEFAULT 0
);

ALTER TABLE public.stock_suppliers ADD COLUMN IF NOT EXISTS clinic_id uuid;
ALTER TABLE public.stock_items
  ADD COLUMN IF NOT EXISTS clinic_id uuid,
  ADD COLUMN IF NOT EXISTS purchase_price double precision NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS stock_kind text NOT NULL DEFAULT 'unknown',
  ADD COLUMN IF NOT EXISTS atk_tax_code text NOT NULL DEFAULT 'C',
  ADD COLUMN IF NOT EXISTS atk_category_code text NOT NULL DEFAULT '',
  ADD COLUMN IF NOT EXISTS atk_unit_code text NOT NULL DEFAULT '',
  ADD COLUMN IF NOT EXISTS sku text,
  ADD COLUMN IF NOT EXISTS barcode text,
  ADD COLUMN IF NOT EXISTS subcategory text,
  ADD COLUMN IF NOT EXISTS purchase_unit text,
  ADD COLUMN IF NOT EXISTS purchase_unit_multiplier double precision NOT NULL DEFAULT 1;

CREATE INDEX IF NOT EXISTS stock_items_sku_idx
  ON public.stock_items (lower(trim(sku)))
  WHERE sku IS NOT NULL AND trim(sku) <> '';
CREATE INDEX IF NOT EXISTS stock_items_barcode_idx
  ON public.stock_items (trim(barcode))
  WHERE barcode IS NOT NULL AND trim(barcode) <> '';
CREATE INDEX IF NOT EXISTS stock_items_clinic_updated_at_idx
  ON public.stock_items (clinic_id, updated_at);

CREATE TABLE IF NOT EXISTS public.stock_movements (
  id uuid PRIMARY KEY,
  clinic_id uuid,
  item_id uuid NOT NULL,
  movement_type text NOT NULL,
  quantity double precision NOT NULL,
  price_per_unit double precision,
  notes text,
  date date NOT NULL,
  created_at timestamptz NOT NULL,
  updated_at timestamptz NOT NULL
);

ALTER TABLE public.stock_movements
  ADD COLUMN IF NOT EXISTS clinic_id uuid,
  ADD COLUMN IF NOT EXISTS updated_at timestamptz;
UPDATE public.stock_movements
  SET updated_at = created_at
  WHERE updated_at IS NULL;
CREATE INDEX IF NOT EXISTS stock_movements_clinic_updated_at_idx
  ON public.stock_movements (clinic_id, updated_at);
