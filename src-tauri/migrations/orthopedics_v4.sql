-- Orthopedics v4: clinician-authorized episode lifecycle and read-only closed records.
-- This is additive and safe to re-run on existing clinic databases.
CREATE TABLE IF NOT EXISTS orthopedic_episode_transitions (
  id TEXT PRIMARY KEY,
  episode_id TEXT NOT NULL REFERENCES orthopedic_episodes(id),
  from_status TEXT NOT NULL CHECK(from_status IN ('active','closed')),
  to_status TEXT NOT NULL CHECK(to_status IN ('active','closed')),
  reason TEXT NOT NULL CHECK(length(trim(reason)) >= 10),
  clinician_id TEXT NOT NULL CHECK(length(trim(clinician_id)) > 0),
  final_report_id TEXT REFERENCES orthopedic_report_versions(id),
  occurred_at TEXT NOT NULL,
  CHECK(from_status <> to_status)
);
CREATE INDEX IF NOT EXISTS idx_ortho_episode_transitions
ON orthopedic_episode_transitions(episode_id,occurred_at);
CREATE TRIGGER IF NOT EXISTS ortho_episode_transition_no_update
BEFORE UPDATE ON orthopedic_episode_transitions BEGIN
 SELECT RAISE(ABORT,'Orthopedic lifecycle history is immutable');
END;
CREATE TRIGGER IF NOT EXISTS ortho_episode_transition_no_delete
BEFORE DELETE ON orthopedic_episode_transitions BEGIN
 SELECT RAISE(ABORT,'Orthopedic lifecycle history is immutable');
END;

CREATE TRIGGER IF NOT EXISTS ortho_implants_closed_insert
BEFORE INSERT ON orthopedic_implants
WHEN (SELECT e.status FROM orthopedic_surgeries s JOIN orthopedic_episodes e ON e.id=s.episode_id WHERE s.id=NEW.surgery_id)='closed'
BEGIN
 SELECT RAISE(ABORT,'Closed orthopedic episode is read only');
END;

CREATE TRIGGER IF NOT EXISTS ortho_implants_closed_update
BEFORE UPDATE ON orthopedic_implants
WHEN (SELECT e.status FROM orthopedic_surgeries s JOIN orthopedic_episodes e ON e.id=s.episode_id WHERE s.id=NEW.surgery_id)='closed' OR (SELECT e.status FROM orthopedic_surgeries s JOIN orthopedic_episodes e ON e.id=s.episode_id WHERE s.id=OLD.surgery_id)='closed'
BEGIN
 SELECT RAISE(ABORT,'Closed orthopedic episode is read only');
END;

CREATE TRIGGER IF NOT EXISTS ortho_implants_closed_delete
BEFORE DELETE ON orthopedic_implants
WHEN (SELECT e.status FROM orthopedic_surgeries s JOIN orthopedic_episodes e ON e.id=s.episode_id WHERE s.id=OLD.surgery_id)='closed'
BEGIN
 SELECT RAISE(ABORT,'Closed orthopedic episode is read only');
END;

CREATE TRIGGER IF NOT EXISTS ortho_implant_events_closed_insert
BEFORE INSERT ON orthopedic_implant_events
WHEN (SELECT e.status FROM orthopedic_implants i JOIN orthopedic_surgeries s ON s.id=i.surgery_id JOIN orthopedic_episodes e ON e.id=s.episode_id WHERE i.id=NEW.implant_id)='closed'
BEGIN
 SELECT RAISE(ABORT,'Closed orthopedic episode is read only');
END;

CREATE TRIGGER IF NOT EXISTS ortho_implant_events_closed_update
BEFORE UPDATE ON orthopedic_implant_events
WHEN (SELECT e.status FROM orthopedic_implants i JOIN orthopedic_surgeries s ON s.id=i.surgery_id JOIN orthopedic_episodes e ON e.id=s.episode_id WHERE i.id=NEW.implant_id)='closed' OR (SELECT e.status FROM orthopedic_implants i JOIN orthopedic_surgeries s ON s.id=i.surgery_id JOIN orthopedic_episodes e ON e.id=s.episode_id WHERE i.id=OLD.implant_id)='closed'
BEGIN
 SELECT RAISE(ABORT,'Closed orthopedic episode is read only');
END;

CREATE TRIGGER IF NOT EXISTS ortho_implant_events_closed_delete
BEFORE DELETE ON orthopedic_implant_events
WHEN (SELECT e.status FROM orthopedic_implants i JOIN orthopedic_surgeries s ON s.id=i.surgery_id JOIN orthopedic_episodes e ON e.id=s.episode_id WHERE i.id=OLD.implant_id)='closed'
BEGIN
 SELECT RAISE(ABORT,'Closed orthopedic episode is read only');
END;

CREATE TRIGGER IF NOT EXISTS ortho_rehab_phase_reviews_closed_insert
BEFORE INSERT ON orthopedic_rehab_phase_reviews
WHEN (SELECT e.status FROM orthopedic_rehab_plans p JOIN orthopedic_episodes e ON e.id=p.episode_id WHERE p.id=NEW.rehab_plan_id)='closed'
BEGIN
 SELECT RAISE(ABORT,'Closed orthopedic episode is read only');
END;

CREATE TRIGGER IF NOT EXISTS ortho_rehab_phase_reviews_closed_update
BEFORE UPDATE ON orthopedic_rehab_phase_reviews
WHEN (SELECT e.status FROM orthopedic_rehab_plans p JOIN orthopedic_episodes e ON e.id=p.episode_id WHERE p.id=NEW.rehab_plan_id)='closed' OR (SELECT e.status FROM orthopedic_rehab_plans p JOIN orthopedic_episodes e ON e.id=p.episode_id WHERE p.id=OLD.rehab_plan_id)='closed'
BEGIN
 SELECT RAISE(ABORT,'Closed orthopedic episode is read only');
END;

CREATE TRIGGER IF NOT EXISTS ortho_rehab_phase_reviews_closed_delete
BEFORE DELETE ON orthopedic_rehab_phase_reviews
WHEN (SELECT e.status FROM orthopedic_rehab_plans p JOIN orthopedic_episodes e ON e.id=p.episode_id WHERE p.id=OLD.rehab_plan_id)='closed'
BEGIN
 SELECT RAISE(ABORT,'Closed orthopedic episode is read only');
END;

CREATE TRIGGER IF NOT EXISTS ortho_assessments_closed_insert
BEFORE INSERT ON orthopedic_assessments
WHEN (SELECT status FROM orthopedic_episodes WHERE id=NEW.episode_id)='closed'
BEGIN
 SELECT RAISE(ABORT,'Closed orthopedic episode is read only');
END;

CREATE TRIGGER IF NOT EXISTS ortho_assessments_closed_update
BEFORE UPDATE ON orthopedic_assessments
WHEN (SELECT status FROM orthopedic_episodes WHERE id=NEW.episode_id)='closed' OR (SELECT status FROM orthopedic_episodes WHERE id=OLD.episode_id)='closed'
BEGIN
 SELECT RAISE(ABORT,'Closed orthopedic episode is read only');
END;

CREATE TRIGGER IF NOT EXISTS ortho_assessments_closed_delete
BEFORE DELETE ON orthopedic_assessments
WHEN (SELECT status FROM orthopedic_episodes WHERE id=OLD.episode_id)='closed'
BEGIN
 SELECT RAISE(ABORT,'Closed orthopedic episode is read only');
END;

CREATE TRIGGER IF NOT EXISTS ortho_fractures_closed_insert
BEFORE INSERT ON orthopedic_fractures
WHEN (SELECT status FROM orthopedic_episodes WHERE id=NEW.episode_id)='closed'
BEGIN
 SELECT RAISE(ABORT,'Closed orthopedic episode is read only');
END;

CREATE TRIGGER IF NOT EXISTS ortho_fractures_closed_update
BEFORE UPDATE ON orthopedic_fractures
WHEN (SELECT status FROM orthopedic_episodes WHERE id=NEW.episode_id)='closed' OR (SELECT status FROM orthopedic_episodes WHERE id=OLD.episode_id)='closed'
BEGIN
 SELECT RAISE(ABORT,'Closed orthopedic episode is read only');
END;

CREATE TRIGGER IF NOT EXISTS ortho_fractures_closed_delete
BEFORE DELETE ON orthopedic_fractures
WHEN (SELECT status FROM orthopedic_episodes WHERE id=OLD.episode_id)='closed'
BEGIN
 SELECT RAISE(ABORT,'Closed orthopedic episode is read only');
END;

CREATE TRIGGER IF NOT EXISTS ortho_surgeries_closed_insert
BEFORE INSERT ON orthopedic_surgeries
WHEN (SELECT status FROM orthopedic_episodes WHERE id=NEW.episode_id)='closed'
BEGIN
 SELECT RAISE(ABORT,'Closed orthopedic episode is read only');
END;

CREATE TRIGGER IF NOT EXISTS ortho_surgeries_closed_update
BEFORE UPDATE ON orthopedic_surgeries
WHEN (SELECT status FROM orthopedic_episodes WHERE id=NEW.episode_id)='closed' OR (SELECT status FROM orthopedic_episodes WHERE id=OLD.episode_id)='closed'
BEGIN
 SELECT RAISE(ABORT,'Closed orthopedic episode is read only');
END;

CREATE TRIGGER IF NOT EXISTS ortho_surgeries_closed_delete
BEFORE DELETE ON orthopedic_surgeries
WHEN (SELECT status FROM orthopedic_episodes WHERE id=OLD.episode_id)='closed'
BEGIN
 SELECT RAISE(ABORT,'Closed orthopedic episode is read only');
END;

CREATE TRIGGER IF NOT EXISTS ortho_rehab_plans_closed_insert
BEFORE INSERT ON orthopedic_rehab_plans
WHEN (SELECT status FROM orthopedic_episodes WHERE id=NEW.episode_id)='closed'
BEGIN
 SELECT RAISE(ABORT,'Closed orthopedic episode is read only');
END;

CREATE TRIGGER IF NOT EXISTS ortho_rehab_plans_closed_update
BEFORE UPDATE ON orthopedic_rehab_plans
WHEN (SELECT status FROM orthopedic_episodes WHERE id=NEW.episode_id)='closed' OR (SELECT status FROM orthopedic_episodes WHERE id=OLD.episode_id)='closed'
BEGIN
 SELECT RAISE(ABORT,'Closed orthopedic episode is read only');
END;

CREATE TRIGGER IF NOT EXISTS ortho_rehab_plans_closed_delete
BEFORE DELETE ON orthopedic_rehab_plans
WHEN (SELECT status FROM orthopedic_episodes WHERE id=OLD.episode_id)='closed'
BEGIN
 SELECT RAISE(ABORT,'Closed orthopedic episode is read only');
END;

CREATE TRIGGER IF NOT EXISTS ortho_markers_closed_insert
BEFORE INSERT ON orthopedic_markers
WHEN (SELECT status FROM orthopedic_episodes WHERE id=NEW.episode_id)='closed'
BEGIN
 SELECT RAISE(ABORT,'Closed orthopedic episode is read only');
END;

CREATE TRIGGER IF NOT EXISTS ortho_markers_closed_update
BEFORE UPDATE ON orthopedic_markers
WHEN (SELECT status FROM orthopedic_episodes WHERE id=NEW.episode_id)='closed' OR (SELECT status FROM orthopedic_episodes WHERE id=OLD.episode_id)='closed'
BEGIN
 SELECT RAISE(ABORT,'Closed orthopedic episode is read only');
END;

CREATE TRIGGER IF NOT EXISTS ortho_markers_closed_delete
BEFORE DELETE ON orthopedic_markers
WHEN (SELECT status FROM orthopedic_episodes WHERE id=OLD.episode_id)='closed'
BEGIN
 SELECT RAISE(ABORT,'Closed orthopedic episode is read only');
END;

CREATE TRIGGER IF NOT EXISTS ortho_imaging_links_closed_insert
BEFORE INSERT ON orthopedic_imaging_links
WHEN (SELECT status FROM orthopedic_episodes WHERE id=NEW.episode_id)='closed'
BEGIN
 SELECT RAISE(ABORT,'Closed orthopedic episode is read only');
END;

CREATE TRIGGER IF NOT EXISTS ortho_imaging_links_closed_update
BEFORE UPDATE ON orthopedic_imaging_links
WHEN (SELECT status FROM orthopedic_episodes WHERE id=NEW.episode_id)='closed' OR (SELECT status FROM orthopedic_episodes WHERE id=OLD.episode_id)='closed'
BEGIN
 SELECT RAISE(ABORT,'Closed orthopedic episode is read only');
END;

CREATE TRIGGER IF NOT EXISTS ortho_imaging_links_closed_delete
BEFORE DELETE ON orthopedic_imaging_links
WHEN (SELECT status FROM orthopedic_episodes WHERE id=OLD.episode_id)='closed'
BEGIN
 SELECT RAISE(ABORT,'Closed orthopedic episode is read only');
END;

CREATE TRIGGER IF NOT EXISTS ortho_postop_visits_closed_insert
BEFORE INSERT ON orthopedic_postop_visits
WHEN (SELECT status FROM orthopedic_episodes WHERE id=NEW.episode_id)='closed'
BEGIN
 SELECT RAISE(ABORT,'Closed orthopedic episode is read only');
END;

CREATE TRIGGER IF NOT EXISTS ortho_postop_visits_closed_update
BEFORE UPDATE ON orthopedic_postop_visits
WHEN (SELECT status FROM orthopedic_episodes WHERE id=NEW.episode_id)='closed' OR (SELECT status FROM orthopedic_episodes WHERE id=OLD.episode_id)='closed'
BEGIN
 SELECT RAISE(ABORT,'Closed orthopedic episode is read only');
END;

CREATE TRIGGER IF NOT EXISTS ortho_postop_visits_closed_delete
BEFORE DELETE ON orthopedic_postop_visits
WHEN (SELECT status FROM orthopedic_episodes WHERE id=OLD.episode_id)='closed'
BEGIN
 SELECT RAISE(ABORT,'Closed orthopedic episode is read only');
END;

CREATE TRIGGER IF NOT EXISTS ortho_fracture_followups_closed_insert
BEFORE INSERT ON orthopedic_fracture_followups
WHEN (SELECT status FROM orthopedic_episodes WHERE id=NEW.episode_id)='closed'
BEGIN
 SELECT RAISE(ABORT,'Closed orthopedic episode is read only');
END;

CREATE TRIGGER IF NOT EXISTS ortho_fracture_followups_closed_update
BEFORE UPDATE ON orthopedic_fracture_followups
WHEN (SELECT status FROM orthopedic_episodes WHERE id=NEW.episode_id)='closed' OR (SELECT status FROM orthopedic_episodes WHERE id=OLD.episode_id)='closed'
BEGIN
 SELECT RAISE(ABORT,'Closed orthopedic episode is read only');
END;

CREATE TRIGGER IF NOT EXISTS ortho_fracture_followups_closed_delete
BEFORE DELETE ON orthopedic_fracture_followups
WHEN (SELECT status FROM orthopedic_episodes WHERE id=OLD.episode_id)='closed'
BEGIN
 SELECT RAISE(ABORT,'Closed orthopedic episode is read only');
END;

CREATE TRIGGER IF NOT EXISTS ortho_operations_closed_insert
BEFORE INSERT ON orthopedic_operations
WHEN (SELECT status FROM orthopedic_episodes WHERE id=NEW.episode_id)='closed'
BEGIN
 SELECT RAISE(ABORT,'Closed orthopedic episode is read only');
END;

CREATE TRIGGER IF NOT EXISTS ortho_operations_closed_update
BEFORE UPDATE ON orthopedic_operations
WHEN (SELECT status FROM orthopedic_episodes WHERE id=NEW.episode_id)='closed' OR (SELECT status FROM orthopedic_episodes WHERE id=OLD.episode_id)='closed'
BEGIN
 SELECT RAISE(ABORT,'Closed orthopedic episode is read only');
END;

CREATE TRIGGER IF NOT EXISTS ortho_operations_closed_delete
BEFORE DELETE ON orthopedic_operations
WHEN (SELECT status FROM orthopedic_episodes WHERE id=OLD.episode_id)='closed'
BEGIN
 SELECT RAISE(ABORT,'Closed orthopedic episode is read only');
END;

-- A signed amendment remains allowed after discharge; routine reports do not.
CREATE TRIGGER IF NOT EXISTS ortho_report_closed_insert
BEFORE INSERT ON orthopedic_report_versions
WHEN NEW.status <> 'amendment' AND
 (SELECT status FROM orthopedic_episodes WHERE id=NEW.episode_id)='closed'
BEGIN SELECT RAISE(ABORT,'Closed episode accepts signed amendments only'); END;
CREATE TRIGGER IF NOT EXISTS ortho_report_closed_update
BEFORE UPDATE ON orthopedic_report_versions
WHEN (SELECT status FROM orthopedic_episodes WHERE id=OLD.episode_id)='closed'
 OR (SELECT status FROM orthopedic_episodes WHERE id=NEW.episode_id)='closed'
BEGIN SELECT RAISE(ABORT,'Closed episode report cannot be overwritten'); END;
CREATE TRIGGER IF NOT EXISTS ortho_report_closed_delete
BEFORE DELETE ON orthopedic_report_versions
WHEN (SELECT status FROM orthopedic_episodes WHERE id=OLD.episode_id)='closed'
BEGIN SELECT RAISE(ABORT,'Closed episode report cannot be deleted'); END;
