-- Local-only fiscal transaction mirror for PROGRAMERI POS SEF v1.
-- Never synchronized to clinical Supabase; never contains patient/diagnosis data.
CREATE TABLE IF NOT EXISTS sef_mjeku_transactions (
 sale_id TEXT PRIMARY KEY REFERENCES sales(id),
 request_sha256 TEXT NOT NULL CHECK(length(request_sha256)=64),
 request_json TEXT NOT NULL CHECK(length(request_json)>2),
 allow_any INTEGER NOT NULL CHECK(allow_any IN (0,1)),
 total_cents INTEGER NOT NULL CHECK(total_cents>0),
 context_id TEXT NOT NULL DEFAULT '',
 status TEXT NOT NULL CHECK(status IN
  ('READY','IN_FLIGHT','UNKNOWN','FISCALIZED','PENDING_OFFLINE','FAILED')),
 coupon_id TEXT,
 daily_number INTEGER,
 error_code TEXT,
 created_at TEXT NOT NULL,
 updated_at TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_sef_mjeku_coupon
 ON sef_mjeku_transactions(context_id,coupon_id)
 WHERE coupon_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_sef_mjeku_status
 ON sef_mjeku_transactions(status,updated_at);
CREATE TRIGGER IF NOT EXISTS sef_mjeku_snapshot_immutable
BEFORE UPDATE OF sale_id,request_sha256,request_json,allow_any,total_cents,context_id
ON sef_mjeku_transactions
BEGIN SELECT RAISE(ABORT,'SEF commercial snapshot is immutable'); END;
CREATE TRIGGER IF NOT EXISTS sef_mjeku_no_delete
BEFORE DELETE ON sef_mjeku_transactions
BEGIN SELECT RAISE(ABORT,'SEF fiscal attempts cannot be deleted'); END;
CREATE TRIGGER IF NOT EXISTS sef_mjeku_coupon_immutable
BEFORE UPDATE OF coupon_id ON sef_mjeku_transactions
WHEN OLD.coupon_id IS NOT NULL AND OLD.coupon_id IS NOT NEW.coupon_id
BEGIN SELECT RAISE(ABORT,'SEF coupon identity cannot change'); END;

CREATE TABLE IF NOT EXISTS sef_mjeku_events (
 id INTEGER PRIMARY KEY AUTOINCREMENT,
 sale_id TEXT NOT NULL REFERENCES sef_mjeku_transactions(sale_id),
 event_type TEXT NOT NULL,
 from_status TEXT,
 to_status TEXT NOT NULL,
 error_code TEXT,
 occurred_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_sef_mjeku_events_sale
 ON sef_mjeku_events(sale_id,id);
CREATE TRIGGER IF NOT EXISTS sef_mjeku_events_no_update
BEFORE UPDATE ON sef_mjeku_events
BEGIN SELECT RAISE(ABORT,'SEF audit events are immutable'); END;
CREATE TRIGGER IF NOT EXISTS sef_mjeku_events_no_delete
BEFORE DELETE ON sef_mjeku_events
BEGIN SELECT RAISE(ABORT,'SEF audit events cannot be deleted'); END;
