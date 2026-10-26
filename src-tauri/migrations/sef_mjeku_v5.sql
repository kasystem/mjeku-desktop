-- Tamper-evident SHA-256 chain for NEW MJEKU fiscal audit events after this migration.
-- Historical v1-v3 events predate this feature and are not retroactively attested.
CREATE TABLE IF NOT EXISTS sef_mjeku_event_chain (
 sequence_no INTEGER PRIMARY KEY AUTOINCREMENT,
 source_kind TEXT NOT NULL CHECK(source_kind IN ('SALE','ACTION')),
 source_event_id INTEGER NOT NULL,
 previous_hash TEXT NOT NULL CHECK(length(previous_hash)=64),
 event_hash TEXT NOT NULL CHECK(length(event_hash)=64),
 created_at TEXT NOT NULL,
 UNIQUE(source_kind,source_event_id)
);
CREATE TRIGGER IF NOT EXISTS sef_mjeku_event_chain_no_update
 BEFORE UPDATE ON sef_mjeku_event_chain
 BEGIN SELECT RAISE(ABORT,'SEF audit hash chain is immutable'); END;
CREATE TRIGGER IF NOT EXISTS sef_mjeku_event_chain_no_delete
 BEFORE DELETE ON sef_mjeku_event_chain
 BEGIN SELECT RAISE(ABORT,'SEF audit hash chain cannot be deleted'); END;
