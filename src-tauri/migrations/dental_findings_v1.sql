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

-- Immutable periodontal examination snapshots, six sites per FDI tooth.
CREATE TABLE IF NOT EXISTS dental_periodontal_exams_v1 (
 id TEXT PRIMARY KEY,
 patient_id TEXT NOT NULL REFERENCES clients(id),
 visit_id TEXT NOT NULL REFERENCES visits(id),
 clinician_id TEXT NOT NULL,
 dentition TEXT NOT NULL CHECK(dentition IN ('permanent','primary')),
 recorded_at TEXT NOT NULL,
 notes TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_dental_perio_patient_date ON dental_periodontal_exams_v1(patient_id,recorded_at);
CREATE TABLE IF NOT EXISTS dental_periodontal_sites_v1 (
 exam_id TEXT NOT NULL REFERENCES dental_periodontal_exams_v1(id),
 tooth_fdi TEXT NOT NULL,
 site TEXT NOT NULL CHECK(site IN ('MB','B','DB','ML','L','DL')),
 probing_depth_mm REAL CHECK(probing_depth_mm IS NULL OR (probing_depth_mm>=0 AND probing_depth_mm<=30)),
 gingival_margin_mm REAL CHECK(gingival_margin_mm IS NULL OR (gingival_margin_mm>=-30 AND gingival_margin_mm<=30)),
 bleeding INTEGER CHECK(bleeding IS NULL OR bleeding IN (0,1)),
 plaque INTEGER CHECK(plaque IS NULL OR plaque IN (0,1)),
 suppuration INTEGER CHECK(suppuration IS NULL OR suppuration IN (0,1)),
 PRIMARY KEY(exam_id,tooth_fdi,site)
);

-- Dental treatment plans: versioned proposals, immutable approval events and line-item snapshots.
CREATE TABLE IF NOT EXISTS dental_treatment_plans_v1 (
 id TEXT PRIMARY KEY,
 patient_id TEXT NOT NULL REFERENCES clients(id),
 visit_id TEXT NOT NULL REFERENCES visits(id),
 clinician_id TEXT NOT NULL,
 version INTEGER NOT NULL DEFAULT 1 CHECK(version >= 1),
 status TEXT NOT NULL DEFAULT 'draft' CHECK(status IN ('draft','proposed','approved','declined','cancelled')),
 title TEXT NOT NULL CHECK(length(trim(title)) > 0),
 currency TEXT NOT NULL DEFAULT 'EUR' CHECK(currency = 'EUR'),
 notes TEXT NOT NULL DEFAULT '',
 created_at TEXT NOT NULL,
 updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_dental_treatment_plans_patient ON dental_treatment_plans_v1(patient_id,created_at);
CREATE TABLE IF NOT EXISTS dental_treatment_plan_items_v1 (
 id TEXT PRIMARY KEY,
 plan_id TEXT NOT NULL REFERENCES dental_treatment_plans_v1(id),
 phase_number INTEGER NOT NULL CHECK(phase_number >= 1),
 tooth_fdi TEXT,
 procedure_name TEXT NOT NULL CHECK(length(trim(procedure_name)) > 0),
 quantity REAL NOT NULL CHECK(quantity > 0),
 unit_price_cents INTEGER NOT NULL CHECK(unit_price_cents >= 0),
 position INTEGER NOT NULL CHECK(position >= 0),
 notes TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_dental_treatment_items_plan ON dental_treatment_plan_items_v1(plan_id,phase_number,position);
CREATE TABLE IF NOT EXISTS dental_treatment_plan_events_v1 (
 id TEXT PRIMARY KEY,
 plan_id TEXT NOT NULL REFERENCES dental_treatment_plans_v1(id),
 actor_id TEXT NOT NULL,
 event_type TEXT NOT NULL CHECK(event_type IN ('created','proposed','approved','declined','cancelled')),
 reason TEXT NOT NULL DEFAULT '',
 occurred_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_dental_treatment_events_plan ON dental_treatment_plan_events_v1(plan_id,occurred_at);
