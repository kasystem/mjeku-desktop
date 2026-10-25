-- Physiotherapy v2: clinical assessments, authored treatment plans,
-- approved home exercises, observations and versioned clinical reports.
-- Only append-only clinical history; never reassign patient or source episode.
CREATE TABLE IF NOT EXISTS physiotherapy_measurements (
 id TEXT PRIMARY KEY,
 physiotherapy_episode_id TEXT NOT NULL REFERENCES physiotherapy_episodes(id),
 session_id TEXT REFERENCES physiotherapy_sessions(id),
 kind TEXT NOT NULL CHECK(kind IN ('rom','mmt','nrs','functional')),
 region TEXT NOT NULL DEFAULT '',
 laterality TEXT NOT NULL CHECK(laterality IN ('left','right','bilateral','midline','not_applicable')),
 movement TEXT NOT NULL DEFAULT '',
 value REAL NOT NULL CHECK(value >= 0 AND value < 1000000),
 unit TEXT NOT NULL,
 method TEXT NOT NULL,
 instrument TEXT NOT NULL DEFAULT '',
 instrument_version TEXT NOT NULL DEFAULT '',
 context TEXT NOT NULL DEFAULT '',
 interpretation TEXT NOT NULL DEFAULT '',
 assessor_id TEXT NOT NULL,
 assessed_at TEXT NOT NULL,
 correction_of TEXT REFERENCES physiotherapy_measurements(id),
 correction_reason TEXT NOT NULL DEFAULT '',
 created_at TEXT NOT NULL,
 CHECK(
 (kind='rom' AND value <= 360 AND unit='deg' AND length(trim(region))>0 AND length(trim(movement))>0)
 OR (kind='mmt' AND value <= 5 AND value=CAST(value AS INTEGER) AND unit='grade_0_5' AND length(trim(region))>0)
 OR (kind='nrs' AND value <= 10 AND value=CAST(value AS INTEGER) AND unit='nrs_0_10')
 OR (kind='functional' AND length(trim(instrument))>0 AND length(trim(instrument_version))>0 AND length(trim(unit))>0)
 ),
 CHECK ((correction_of IS NULL AND correction_reason='') OR
        (correction_of IS NOT NULL AND length(trim(correction_reason))>=10))
);
CREATE INDEX IF NOT EXISTS idx_physio_measurement_time ON physiotherapy_measurements(
 physiotherapy_episode_id,kind,assessed_at
);
CREATE TRIGGER IF NOT EXISTS physio_measurement_no_update BEFORE UPDATE
 ON physiotherapy_measurements BEGIN
 SELECT RAISE(ABORT,'Clinical measurements are append-only'); END;
CREATE TRIGGER IF NOT EXISTS physio_measurement_no_delete BEFORE DELETE
 ON physiotherapy_measurements BEGIN
 SELECT RAISE(ABORT,'Clinical measurements cannot be deleted'); END;
CREATE TRIGGER IF NOT EXISTS physio_measurement_validate_episode BEFORE INSERT
 ON physiotherapy_measurements
 WHEN NOT EXISTS (
 SELECT 1 FROM physiotherapy_episodes f
 JOIN orthopedic_episodes e ON e.id=f.orthopedic_episode_id
 WHERE f.id=NEW.physiotherapy_episode_id AND e.status='active' AND f.status='active')
 OR (NEW.session_id IS NOT NULL AND NOT EXISTS (
 SELECT 1 FROM physiotherapy_sessions s
 WHERE s.id=NEW.session_id AND s.physiotherapy_episode_id=NEW.physiotherapy_episode_id))
 OR (NEW.correction_of IS NOT NULL AND NOT EXISTS (
 SELECT 1 FROM physiotherapy_measurements m
 WHERE m.id=NEW.correction_of AND m.physiotherapy_episode_id=NEW.physiotherapy_episode_id
 AND m.kind=NEW.kind))
 BEGIN SELECT RAISE(ABORT,'Invalid physiotherapy measurement episode or correction'); END;

CREATE TABLE IF NOT EXISTS physiotherapy_care_plans (
 id TEXT PRIMARY KEY,
 physiotherapy_episode_id TEXT NOT NULL REFERENCES physiotherapy_episodes(id),
 version INTEGER NOT NULL CHECK(version>0),
 status TEXT NOT NULL CHECK(status IN ('draft','approved','amendment')),
 goal TEXT NOT NULL,
 measurable_target TEXT NOT NULL DEFAULT '',
 frequency TEXT NOT NULL DEFAULT '',
 duration TEXT NOT NULL DEFAULT '',
 interventions TEXT NOT NULL DEFAULT '',
 precautions TEXT NOT NULL DEFAULT '',
 phase TEXT NOT NULL DEFAULT '',
 clinician_id TEXT NOT NULL,
 reason TEXT NOT NULL DEFAULT '',
 created_at TEXT NOT NULL,
 UNIQUE(physiotherapy_episode_id,version),
 CHECK(status<>'amendment' OR length(trim(reason))>=10)
);
CREATE INDEX IF NOT EXISTS idx_physio_care_plan_episode
 ON physiotherapy_care_plans(physiotherapy_episode_id,version DESC);
CREATE TRIGGER IF NOT EXISTS physio_care_plan_no_update BEFORE UPDATE
 ON physiotherapy_care_plans BEGIN SELECT RAISE(ABORT,'Care plans are versioned and immutable'); END;
CREATE TRIGGER IF NOT EXISTS physio_care_plan_no_delete BEFORE DELETE
 ON physiotherapy_care_plans BEGIN SELECT RAISE(ABORT,'Care plan history cannot be deleted'); END;
CREATE TRIGGER IF NOT EXISTS physio_care_plan_active BEFORE INSERT ON physiotherapy_care_plans
 WHEN NOT EXISTS (
 SELECT 1 FROM physiotherapy_episodes f JOIN orthopedic_episodes e
 ON e.id=f.orthopedic_episode_id WHERE f.id=NEW.physiotherapy_episode_id
 AND e.status='active' AND f.status='active')
 BEGIN SELECT RAISE(ABORT,'Cannot author plan for inactive orthopedic episode'); END;

CREATE TABLE IF NOT EXISTS physiotherapy_exercises (
 id TEXT PRIMARY KEY,
 title TEXT NOT NULL CHECK(length(trim(title))>=3),
 anatomical_region TEXT NOT NULL DEFAULT '',
 instructions TEXT NOT NULL,
 media_ref TEXT NOT NULL DEFAULT '',
 media_rights TEXT NOT NULL DEFAULT '',
 contraindications TEXT NOT NULL DEFAULT '',
 version INTEGER NOT NULL DEFAULT 1 CHECK(version>0),
 created_by TEXT NOT NULL,
 created_at TEXT NOT NULL
);
CREATE TRIGGER IF NOT EXISTS physio_exercise_no_update BEFORE UPDATE
 ON physiotherapy_exercises BEGIN SELECT RAISE(ABORT,'Exercise versions are immutable'); END;
CREATE TRIGGER IF NOT EXISTS physio_exercise_no_delete BEFORE DELETE
 ON physiotherapy_exercises BEGIN SELECT RAISE(ABORT,'Exercise catalog cannot be deleted'); END;

CREATE TABLE IF NOT EXISTS physiotherapy_hep (
 id TEXT PRIMARY KEY,
 physiotherapy_episode_id TEXT NOT NULL REFERENCES physiotherapy_episodes(id),
 care_plan_id TEXT NOT NULL REFERENCES physiotherapy_care_plans(id),
 exercise_id TEXT NOT NULL REFERENCES physiotherapy_exercises(id),
 sets INTEGER NOT NULL CHECK(sets BETWEEN 1 AND 30),
 repetitions INTEGER NOT NULL CHECK(repetitions BETWEEN 1 AND 500),
 frequency TEXT NOT NULL,
 duration_seconds INTEGER CHECK(duration_seconds IS NULL OR duration_seconds BETWEEN 1 AND 36000),
 instructions TEXT NOT NULL DEFAULT '',
 authorized_by TEXT NOT NULL,
 load_restrictions_snapshot TEXT NOT NULL,
 rom_restrictions_snapshot TEXT NOT NULL,
 status TEXT NOT NULL CHECK(status IN ('approved','withdrawn')),
 replaces_id TEXT REFERENCES physiotherapy_hep(id),
 created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_physio_hep_epi ON physiotherapy_hep(
 physiotherapy_episode_id,created_at);
CREATE TRIGGER IF NOT EXISTS physio_hep_no_update BEFORE UPDATE
 ON physiotherapy_hep BEGIN SELECT RAISE(ABORT,'Exercise prescriptions are immutable'); END;
CREATE TRIGGER IF NOT EXISTS physio_hep_no_delete BEFORE DELETE
 ON physiotherapy_hep BEGIN SELECT RAISE(ABORT,'Exercise prescription history cannot be deleted'); END;
CREATE TRIGGER IF NOT EXISTS physio_hep_integrity BEFORE INSERT ON physiotherapy_hep
 WHEN NOT EXISTS (
 SELECT 1 FROM physiotherapy_episodes f
 JOIN orthopedic_episodes e ON e.id=f.orthopedic_episode_id
 JOIN orthopedic_rehab_plans r ON r.id=f.rehab_plan_id
 JOIN physiotherapy_care_plans p ON p.physiotherapy_episode_id=f.id
 WHERE f.id=NEW.physiotherapy_episode_id AND p.id=NEW.care_plan_id
 AND p.status IN ('approved','amendment') AND e.status='active' AND f.status='active'
 AND COALESCE(r.load_restrictions,'')=NEW.load_restrictions_snapshot
 AND COALESCE(r.rom_restrictions,'')=NEW.rom_restrictions_snapshot)
 BEGIN SELECT RAISE(ABORT,'HEP needs approved clinical plan and current restrictions'); END;

CREATE TABLE IF NOT EXISTS physiotherapy_hep_logs (
 id TEXT PRIMARY KEY,
 hep_id TEXT NOT NULL REFERENCES physiotherapy_hep(id),
 performed_date TEXT NOT NULL,
 self_reported INTEGER NOT NULL CHECK(self_reported IN (0,1)),
 performed INTEGER NOT NULL CHECK(performed IN (0,1)),
 response TEXT NOT NULL DEFAULT '',
 recorded_by TEXT NOT NULL,
 created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_physio_hep_log ON physiotherapy_hep_logs(hep_id,performed_date);
CREATE TRIGGER IF NOT EXISTS physio_hep_log_no_update BEFORE UPDATE
 ON physiotherapy_hep_logs BEGIN SELECT RAISE(ABORT,'HEP adherence logs are immutable'); END;
CREATE TRIGGER IF NOT EXISTS physio_hep_log_no_delete BEFORE DELETE
 ON physiotherapy_hep_logs BEGIN SELECT RAISE(ABORT,'HEP adherence logs cannot be deleted'); END;

CREATE TABLE IF NOT EXISTS physiotherapy_reports (
 id TEXT PRIMARY KEY,
 physiotherapy_episode_id TEXT NOT NULL REFERENCES physiotherapy_episodes(id),
 type TEXT NOT NULL CHECK(type IN ('initial','progress','discharge')),
 version INTEGER NOT NULL CHECK(version>0),
 status TEXT NOT NULL CHECK(status IN ('draft','final','amendment')),
 narrative TEXT NOT NULL,
 clinical_summary_json TEXT NOT NULL DEFAULT '{}',
 author_id TEXT NOT NULL,
 reason TEXT NOT NULL DEFAULT '',
 created_at TEXT NOT NULL,
 UNIQUE(physiotherapy_episode_id,type,version),
 CHECK(status<>'amendment' OR length(trim(reason))>=10)
);
CREATE INDEX IF NOT EXISTS idx_physio_reports_epi ON physiotherapy_reports(
 physiotherapy_episode_id,type,version DESC);
CREATE TRIGGER IF NOT EXISTS physio_report_no_update BEFORE UPDATE
 ON physiotherapy_reports BEGIN SELECT RAISE(ABORT,'Clinical report versions cannot be overwritten'); END;
CREATE TRIGGER IF NOT EXISTS physio_report_no_delete BEFORE DELETE
 ON physiotherapy_reports BEGIN SELECT RAISE(ABORT,'Clinical report history cannot be deleted'); END;
