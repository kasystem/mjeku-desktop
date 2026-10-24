-- Additive, idempotent clinical storage for Orthopedics.
CREATE TABLE IF NOT EXISTS orthopedic_episodes (
  id TEXT PRIMARY KEY,
  client_id TEXT NOT NULL,
  diagnosis TEXT NOT NULL,
  region TEXT NOT NULL,
  laterality TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'active',
  notes TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_orthopedic_episodes_client ON orthopedic_episodes(client_id);

CREATE TABLE IF NOT EXISTS orthopedic_assessments (
  id TEXT PRIMARY KEY,
  episode_id TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_orthopedic_assessments_episode ON orthopedic_assessments(episode_id);

CREATE TABLE IF NOT EXISTS orthopedic_fractures (
  id TEXT PRIMARY KEY,
  episode_id TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS orthopedic_surgeries (
  id TEXT PRIMARY KEY,
  episode_id TEXT NOT NULL,
  procedure TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'draft',
  payload_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS orthopedic_implants (
  id TEXT PRIMARY KEY,
  surgery_id TEXT NOT NULL,
  udi TEXT NOT NULL DEFAULT '',
  lot TEXT NOT NULL DEFAULT '',
  serial TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'reserved',
  payload_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS orthopedic_rehab_plans (
  id TEXT PRIMARY KEY,
  episode_id TEXT NOT NULL,
  protocol TEXT NOT NULL,
  phase TEXT NOT NULL,
  payload_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS orthopedic_audit_log (
  id TEXT PRIMARY KEY,
  entity_type TEXT NOT NULL,
  entity_id TEXT NOT NULL,
  action TEXT NOT NULL,
  actor TEXT NOT NULL DEFAULT '',
  payload_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_orthopedic_audit_entity ON orthopedic_audit_log(entity_type, entity_id);
