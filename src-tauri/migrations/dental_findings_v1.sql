-- Dental finding ledger. Additive and idempotent, shares existing clients/visits.
CREATE TABLE IF NOT EXISTS dental_findings_v1 (
 id TEXT PRIMARY KEY,
 patient_id TEXT NOT NULL REFERENCES clients(id),
 visit_id TEXT NOT NULL REFERENCES visits(id),
 clinician_id TEXT NOT NULL,
 tooth_fdi TEXT NOT NULL,
 surface TEXT,
 kind TEXT NOT NULL,
 status TEXT NOT NULL,
 notes TEXT,
 recorded_at TEXT NOT NULL,
 CHECK(status IN ('existing','planned','completed')),
 CHECK(surface IS NULL OR surface IN ('M','D','B','L','O','I'))
);
CREATE INDEX IF NOT EXISTS idx_dental_findings_patient_tooth ON dental_findings_v1(patient_id,tooth_fdi,recorded_at);
CREATE TABLE IF NOT EXISTS dental_finding_events_v1 (
 id TEXT PRIMARY KEY, finding_id TEXT NOT NULL REFERENCES dental_findings_v1(id),
 actor_id TEXT NOT NULL, event_type TEXT NOT NULL,
 occurred_at TEXT NOT NULL, payload_json TEXT NOT NULL
);

-- Corrections are append-only; the original finding is never overwritten.
CREATE TABLE IF NOT EXISTS dental_finding_corrections_v1 (
 id TEXT PRIMARY KEY,
 finding_id TEXT NOT NULL REFERENCES dental_findings_v1(id),
 actor_id TEXT NOT NULL,
 reason TEXT NOT NULL CHECK(length(trim(reason))>0),
 corrected_kind TEXT NOT NULL,
 corrected_status TEXT NOT NULL CHECK(corrected_status IN ('existing','planned','completed')),
 corrected_surface TEXT CHECK(corrected_surface IS NULL OR corrected_surface IN ('M','D','B','L','O','I')),
 corrected_notes TEXT,
 corrected_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_dental_finding_corrections_finding ON dental_finding_corrections_v1(finding_id,corrected_at);
