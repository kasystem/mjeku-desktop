-- Physiotherapy v4: final database-level safeguards for home exercise logs,
-- finalized care reports and changes to the clinical plan.
CREATE TRIGGER IF NOT EXISTS physio_hep_log_integrity
BEFORE INSERT ON physiotherapy_hep_logs
WHEN NOT EXISTS(
 SELECT 1 FROM physiotherapy_hep h
 JOIN physiotherapy_episodes f ON f.id=h.physiotherapy_episode_id
 JOIN orthopedic_episodes e ON e.id=f.orthopedic_episode_id
 JOIN orthopedic_rehab_plans p ON p.id=f.rehab_plan_id
 WHERE h.id=NEW.hep_id AND h.status='approved' AND f.status='active'
 AND e.status='active'
 AND h.load_restrictions_snapshot=COALESCE(p.load_restrictions,'')
 AND h.rom_restrictions_snapshot=COALESCE(p.rom_restrictions,'')
 AND h.care_plan_id=(
   SELECT id FROM physiotherapy_care_plans cp
   WHERE cp.physiotherapy_episode_id=f.id ORDER BY version DESC LIMIT 1
 )
 AND NOT EXISTS(SELECT 1 FROM physiotherapy_hep newer
    WHERE newer.replaces_id=h.id)
)
BEGIN SELECT RAISE(ABORT,'HEP observation requires valid current prescription and open clinical episode'); END;

CREATE TRIGGER IF NOT EXISTS physio_report_closed_guard
BEFORE INSERT ON physiotherapy_reports
WHEN NEW.status <> 'amendment' AND
 EXISTS(
  SELECT 1 FROM physiotherapy_episodes f
  JOIN orthopedic_episodes e ON e.id=f.orthopedic_episode_id
  WHERE f.id=NEW.physiotherapy_episode_id AND e.status='closed'
 )
BEGIN SELECT RAISE(ABORT,'Clinical reports on closed cases require signed amendment'); END;

CREATE TRIGGER IF NOT EXISTS physio_hep_closed_guard
BEFORE INSERT ON physiotherapy_hep
WHEN NOT EXISTS(
 SELECT 1 FROM physiotherapy_episodes f
 JOIN orthopedic_episodes e ON e.id=f.orthopedic_episode_id
 WHERE f.id=NEW.physiotherapy_episode_id AND e.status='active'
)
BEGIN SELECT RAISE(ABORT,'Closed case cannot receive new exercise prescription'); END;
