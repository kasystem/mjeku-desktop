-- Additive schema proposal for shared clinical core.
-- Not yet wired into Db::run_migrations; do not run manually on production databases.
CREATE TABLE IF NOT EXISTS clinical_conditions (
 id TEXT PRIMARY KEY, patient_id TEXT NOT NULL REFERENCES clients(id),
 code TEXT NOT NULL, code_system TEXT NOT NULL, code_version TEXT NOT NULL,
 display TEXT NOT NULL, clinical_status TEXT NOT NULL,
 verification_status TEXT NOT NULL, clinician_id TEXT NOT NULL,
 encounter_id TEXT REFERENCES visits(id), onset_date TEXT, resolved_date TEXT,
 notes TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
 CHECK(clinical_status IN ('active','inactive','resolved','history','entered_in_error')),
 CHECK(verification_status IN ('unconfirmed','provisional','confirmed','refuted','entered_in_error'))
);
CREATE INDEX IF NOT EXISTS idx_clinical_conditions_patient ON clinical_conditions(patient_id,clinical_status);
CREATE TABLE IF NOT EXISTS clinical_condition_events (
 id TEXT PRIMARY KEY, condition_id TEXT NOT NULL REFERENCES clinical_conditions(id),
 author_id TEXT NOT NULL, recorded_at TEXT NOT NULL, event_type TEXT NOT NULL,
 old_value TEXT, new_value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS clinical_notes (
 id TEXT PRIMARY KEY, patient_id TEXT NOT NULL REFERENCES clients(id),
 encounter_id TEXT NOT NULL REFERENCES visits(id), author_id TEXT NOT NULL,
 version INTEGER NOT NULL CHECK(version>0), status TEXT NOT NULL,
 subjective TEXT NOT NULL DEFAULT '', objective TEXT NOT NULL DEFAULT '',
 assessment TEXT NOT NULL DEFAULT '', plan TEXT NOT NULL DEFAULT '',
 previous_version_id TEXT REFERENCES clinical_notes(id),
 created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
 CHECK(status IN ('draft','in_review','finalized','amended','entered_in_error'))
);
CREATE INDEX IF NOT EXISTS idx_clinical_notes_encounter ON clinical_notes(encounter_id);
CREATE TABLE IF NOT EXISTS clinical_allergies (
 id TEXT PRIMARY KEY, patient_id TEXT NOT NULL REFERENCES clients(id),
 substance TEXT NOT NULL, category TEXT NOT NULL, verification TEXT NOT NULL,
 reaction TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
 CHECK(category IN ('allergy','intolerance','unknown'))
);
CREATE INDEX IF NOT EXISTS idx_clinical_allergies_patient ON clinical_allergies(patient_id);
CREATE TABLE IF NOT EXISTS clinical_referrals (
 id TEXT PRIMARY KEY, patient_id TEXT NOT NULL REFERENCES clients(id),
 encounter_id TEXT REFERENCES visits(id), clinician_id TEXT NOT NULL,
 destination TEXT NOT NULL, reason TEXT NOT NULL,
 status TEXT NOT NULL, delivery_acknowledged INTEGER NOT NULL DEFAULT 0,
 created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_clinical_referrals_patient ON clinical_referrals(patient_id,status);
CREATE TABLE IF NOT EXISTS clinical_referral_events (
 id TEXT PRIMARY KEY, referral_id TEXT NOT NULL REFERENCES clinical_referrals(id),
 author_id TEXT NOT NULL, old_status TEXT, new_status TEXT NOT NULL,
 occurred_at TEXT NOT NULL, details TEXT NOT NULL DEFAULT ''
);
