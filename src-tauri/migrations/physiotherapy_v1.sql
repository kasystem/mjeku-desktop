-- Shared Physiotherapy integration v1. Patient/orthopedic episode and rehab
-- protocol remain authoritative in existing tables; do not duplicate patients.
CREATE TABLE IF NOT EXISTS physiotherapy_episodes (
  id TEXT PRIMARY KEY,
  client_id TEXT NOT NULL REFERENCES clients(id),
  orthopedic_episode_id TEXT NOT NULL REFERENCES orthopedic_episodes(id),
  rehab_plan_id TEXT NOT NULL UNIQUE REFERENCES orthopedic_rehab_plans(id),
  status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active','completed','paused')),
  created_by TEXT NOT NULL CHECK(length(trim(created_by)) > 0),
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_physio_episodes_patient ON physiotherapy_episodes(client_id);
CREATE INDEX IF NOT EXISTS idx_physio_episodes_ortho ON physiotherapy_episodes(orthopedic_episode_id);
-- A referral is immutable: no patient/plan swapping after clinical creation.
CREATE TRIGGER IF NOT EXISTS physio_episode_no_update
BEFORE UPDATE ON physiotherapy_episodes BEGIN
 SELECT RAISE(ABORT,'Physiotherapy referral is immutable'); END;
CREATE TRIGGER IF NOT EXISTS physio_episode_no_delete
BEFORE DELETE ON physiotherapy_episodes BEGIN
 SELECT RAISE(ABORT,'Physiotherapy referral cannot be deleted'); END;
CREATE TRIGGER IF NOT EXISTS physio_episode_integrity
BEFORE INSERT ON physiotherapy_episodes
WHEN NOT EXISTS (
 SELECT 1 FROM orthopedic_rehab_plans p
 JOIN orthopedic_episodes e ON e.id=p.episode_id
 WHERE e.id=NEW.orthopedic_episode_id AND e.client_id=NEW.client_id
 AND p.id=NEW.rehab_plan_id AND e.status='active'
)
BEGIN SELECT RAISE(ABORT,'Physiotherapy referral does not match patient and active orthopedic rehabilitation plan'); END;

CREATE TABLE IF NOT EXISTS physiotherapy_sessions (
  id TEXT PRIMARY KEY,
  physiotherapy_episode_id TEXT NOT NULL REFERENCES physiotherapy_episodes(id),
  session_date TEXT NOT NULL,
  therapist_id TEXT NOT NULL CHECK(length(trim(therapist_id))>0),
  intervention TEXT NOT NULL CHECK(length(trim(intervention)) >= 3),
  response TEXT NOT NULL DEFAULT '',
  pain_before INTEGER CHECK(pain_before IS NULL OR pain_before BETWEEN 0 AND 10),
  pain_after INTEGER CHECK(pain_after IS NULL OR pain_after BETWEEN 0 AND 10),
  rom_joint TEXT NOT NULL DEFAULT '',
  rom_movement TEXT NOT NULL DEFAULT '',
  rom_degrees REAL CHECK(rom_degrees IS NULL OR rom_degrees BETWEEN 0 AND 360),
  mmt_grade INTEGER CHECK(mmt_grade IS NULL OR mmt_grade BETWEEN 0 AND 5),
  next_plan TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_physio_sessions_episode_date
ON physiotherapy_sessions(physiotherapy_episode_id,session_date,created_at);
CREATE TRIGGER IF NOT EXISTS physio_session_no_update
BEFORE UPDATE ON physiotherapy_sessions BEGIN
 SELECT RAISE(ABORT,'Physiotherapy session is append-only'); END;
CREATE TRIGGER IF NOT EXISTS physio_session_no_delete
BEFORE DELETE ON physiotherapy_sessions BEGIN
 SELECT RAISE(ABORT,'Physiotherapy session cannot be deleted'); END;
CREATE TRIGGER IF NOT EXISTS physio_session_no_closed_orthopedics
BEFORE INSERT ON physiotherapy_sessions WHEN
 (SELECT e.status FROM physiotherapy_episodes f
  JOIN orthopedic_episodes e ON e.id=f.orthopedic_episode_id
  WHERE f.id=NEW.physiotherapy_episode_id) <> 'active'
BEGIN SELECT RAISE(ABORT,'Orthopedic episode is closed; physiotherapy session denied'); END;
