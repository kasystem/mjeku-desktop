-- Additive, backward-compatible Orthopedics & Traumatology clinical history.
-- Append-only records retain evidence and never silently overwrite finalized work.
-- The existing v1/v2 episode and imaging data remains unchanged.
CREATE TABLE IF NOT EXISTS orthopedic_fracture_followups (
  id TEXT PRIMARY KEY,
  fracture_id TEXT NOT NULL REFERENCES orthopedic_fractures(id),
  episode_id TEXT NOT NULL REFERENCES orthopedic_episodes(id),
  assessment_date TEXT NOT NULL,
  clinical_findings TEXT NOT NULL DEFAULT '',
  pain_score INTEGER CHECK (pain_score IS NULL OR pain_score BETWEEN 0 AND 10),
  functional_status TEXT NOT NULL DEFAULT '',
  alignment TEXT NOT NULL DEFAULT '',
  healing_status TEXT NOT NULL DEFAULT 'unknown'
    CHECK (healing_status IN ('unknown','under_treatment','healing','healed','delayed_union','nonunion','malunion')),
  imaging_ref TEXT NOT NULL DEFAULT '',
  complications TEXT NOT NULL DEFAULT '',
  treatment_changes TEXT NOT NULL DEFAULT '',
  next_step TEXT NOT NULL DEFAULT '',
  clinician_id TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_ortho_fracture_followups_fracture_date
  ON orthopedic_fracture_followups(fracture_id,assessment_date);
CREATE TRIGGER IF NOT EXISTS orthopedic_fracture_followup_no_update
BEFORE UPDATE ON orthopedic_fracture_followups BEGIN
  SELECT RAISE(ABORT,'Fracture follow-up is append-only');
END;
CREATE TRIGGER IF NOT EXISTS orthopedic_fracture_followup_no_delete
BEFORE DELETE ON orthopedic_fracture_followups BEGIN
  SELECT RAISE(ABORT,'Fracture follow-up history cannot be deleted');
END;

CREATE TABLE IF NOT EXISTS orthopedic_operations (
  id TEXT PRIMARY KEY,
  plan_id TEXT NOT NULL REFERENCES orthopedic_surgeries(id),
  episode_id TEXT NOT NULL REFERENCES orthopedic_episodes(id),
  actual_procedure TEXT NOT NULL,
  operative_site TEXT NOT NULL,
  laterality TEXT NOT NULL CHECK(laterality IN ('left','right','bilateral','midline','not_applicable')),
  start_at TEXT NOT NULL,
  end_at TEXT NOT NULL,
  surgical_team TEXT NOT NULL,
  findings TEXT NOT NULL DEFAULT '',
  technique TEXT NOT NULL DEFAULT '',
  complications TEXT NOT NULL DEFAULT '',
  operative_note TEXT NOT NULL DEFAULT '',
  postop_instructions TEXT NOT NULL DEFAULT '',
  consent_verified INTEGER NOT NULL CHECK(consent_verified=1),
  patient_verified INTEGER NOT NULL CHECK(patient_verified=1),
  site_verified INTEGER NOT NULL CHECK(site_verified=1),
  laterality_verified INTEGER NOT NULL CHECK(laterality_verified=1),
  surgeon_id TEXT NOT NULL,
  finalized_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_ortho_operations_episode ON orthopedic_operations(episode_id,finalized_at);
CREATE TRIGGER IF NOT EXISTS orthopedic_operation_no_update
BEFORE UPDATE ON orthopedic_operations BEGIN
  SELECT RAISE(ABORT,'Finalized operation cannot be overwritten; create an amendment');
END;
CREATE TRIGGER IF NOT EXISTS orthopedic_operation_no_delete
BEFORE DELETE ON orthopedic_operations BEGIN
  SELECT RAISE(ABORT,'Finalized operative report cannot be deleted');
END;

CREATE TABLE IF NOT EXISTS orthopedic_implant_events (
  id TEXT PRIMARY KEY,
  implant_id TEXT NOT NULL REFERENCES orthopedic_implants(id),
  operation_id TEXT REFERENCES orthopedic_operations(id),
  action TEXT NOT NULL CHECK(action IN ('reserved','implanted','removed','replaced','revised','recalled','corrected')),
  clinician_id TEXT NOT NULL,
  note TEXT NOT NULL DEFAULT '',
  occurred_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_ortho_implant_events_device ON orthopedic_implant_events(implant_id,occurred_at);
CREATE TRIGGER IF NOT EXISTS orthopedic_implant_event_no_update
BEFORE UPDATE ON orthopedic_implant_events BEGIN
  SELECT RAISE(ABORT,'Implant audit event cannot be changed');
END;
CREATE TRIGGER IF NOT EXISTS orthopedic_implant_event_no_delete
BEFORE DELETE ON orthopedic_implant_events BEGIN
  SELECT RAISE(ABORT,'Implant audit event cannot be deleted');
END;

CREATE TABLE IF NOT EXISTS orthopedic_rehab_phase_reviews (
  id TEXT PRIMARY KEY,
  rehab_plan_id TEXT NOT NULL REFERENCES orthopedic_rehab_plans(id),
  phase TEXT NOT NULL,
  criteria TEXT NOT NULL,
  decision TEXT NOT NULL CHECK(decision IN ('continue','advance','hold','stop')),
  clinician_id TEXT NOT NULL,
  reason TEXT NOT NULL,
  reviewed_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_ortho_rehab_phase_reviews_plan ON orthopedic_rehab_phase_reviews(rehab_plan_id,reviewed_at);
CREATE TRIGGER IF NOT EXISTS orthopedic_rehab_review_no_update
BEFORE UPDATE ON orthopedic_rehab_phase_reviews BEGIN
  SELECT RAISE(ABORT,'Rehabilitation phase review cannot be overwritten');
END;

CREATE TABLE IF NOT EXISTS orthopedic_report_versions (
  id TEXT PRIMARY KEY,
  episode_id TEXT NOT NULL REFERENCES orthopedic_episodes(id),
  type TEXT NOT NULL,
  version INTEGER NOT NULL CHECK(version > 0),
  status TEXT NOT NULL CHECK(status IN ('draft','final','amendment')),
  body_json TEXT NOT NULL,
  author_id TEXT NOT NULL,
  reason TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  UNIQUE(episode_id,type,version)
);
CREATE INDEX IF NOT EXISTS idx_ortho_reports_episode ON orthopedic_report_versions(episode_id,type,version);
CREATE TRIGGER IF NOT EXISTS orthopedic_final_report_immutable
BEFORE UPDATE ON orthopedic_report_versions
WHEN OLD.status <> 'draft' BEGIN
  SELECT RAISE(ABORT,'Finalized orthopedic reports are immutable');
END;
CREATE TRIGGER IF NOT EXISTS orthopedic_final_report_no_delete
BEFORE DELETE ON orthopedic_report_versions
WHEN OLD.status <> 'draft' BEGIN
  SELECT RAISE(ABORT,'Finalized orthopedic reports cannot be deleted');
END;
