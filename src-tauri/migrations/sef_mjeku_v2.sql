-- Fiscal action journal: return/cancel attempts only, no patient or clinical data.
-- request_id is generated once and persisted BEFORE crossing the POS named pipe.
CREATE TABLE IF NOT EXISTS sef_mjeku_actions (
 request_id TEXT PRIMARY KEY CHECK(length(request_id)=36),
 kind TEXT NOT NULL CHECK(kind IN ('RETURN','CANCEL')),
 original_coupon_id TEXT NOT NULL CHECK(length(original_coupon_id)>0),
 context_id TEXT NOT NULL CHECK(length(context_id)>0),
 request_sha256 TEXT NOT NULL CHECK(length(request_sha256)=64),
 request_json TEXT NOT NULL CHECK(length(request_json)>2),
 status TEXT NOT NULL CHECK(status IN
   ('READY','IN_FLIGHT','UNKNOWN','FISCALIZED','PENDING_OFFLINE','FAILED')),
 coupon_id TEXT,
 daily_number INTEGER,
 error_code TEXT,
 created_at TEXT NOT NULL,
 updated_at TEXT NOT NULL
);
-- A second fiscal action is prohibited while the first may have crossed the IPC boundary.
CREATE UNIQUE INDEX IF NOT EXISTS idx_sef_mjeku_action_unresolved
 ON sef_mjeku_actions(context_id,original_coupon_id)
 WHERE status IN ('READY','IN_FLIGHT','UNKNOWN');
CREATE INDEX IF NOT EXISTS idx_sef_mjeku_actions_recent
 ON sef_mjeku_actions(updated_at);
CREATE TRIGGER IF NOT EXISTS sef_mjeku_action_snapshot_immutable
 BEFORE UPDATE OF request_id,kind,original_coupon_id,context_id,request_sha256,request_json
 ON sef_mjeku_actions
 BEGIN SELECT RAISE(ABORT,'SEF action snapshot is immutable'); END;
CREATE TRIGGER IF NOT EXISTS sef_mjeku_action_no_delete
 BEFORE DELETE ON sef_mjeku_actions
 BEGIN SELECT RAISE(ABORT,'SEF action journal is append-only'); END;

CREATE TABLE IF NOT EXISTS sef_mjeku_action_events (
 id INTEGER PRIMARY KEY AUTOINCREMENT,
 request_id TEXT NOT NULL REFERENCES sef_mjeku_actions(request_id),
 event_type TEXT NOT NULL,
 from_status TEXT,
 to_status TEXT NOT NULL,
 error_code TEXT,
 occurred_at TEXT NOT NULL
);
CREATE TRIGGER IF NOT EXISTS sef_mjeku_action_event_no_update
 BEFORE UPDATE ON sef_mjeku_action_events
 BEGIN SELECT RAISE(ABORT,'SEF action events are append-only'); END;
CREATE TRIGGER IF NOT EXISTS sef_mjeku_action_event_no_delete
 BEFORE DELETE ON sef_mjeku_action_events
 BEGIN SELECT RAISE(ABORT,'SEF action events are append-only'); END;
