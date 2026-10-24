-- Additive, idempotent clinical storage for Orthopedics (v2).
CREATE TABLE IF NOT EXISTS orthopedic_markers (
  id TEXT PRIMARY KEY,
  episode_id TEXT NOT NULL,
  view TEXT NOT NULL DEFAULT 'anterior',
  region TEXT NOT NULL,
  side TEXT NOT NULL DEFAULT '',
  injury_type TEXT NOT NULL DEFAULT '',
  note TEXT NOT NULL DEFAULT '',
  x REAL NOT NULL,
  y REAL NOT NULL,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_orthopedic_markers_episode ON orthopedic_markers(episode_id);

CREATE TABLE IF NOT EXISTS orthopedic_imaging_links (
  id TEXT PRIMARY KEY,
  episode_id TEXT NOT NULL,
  fracture_id TEXT NOT NULL DEFAULT '',
  study_ref TEXT NOT NULL DEFAULT '',
  modality TEXT NOT NULL DEFAULT '',
  taken_at TEXT NOT NULL DEFAULT '',
  reviewer_note TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_orthopedic_imaging_episode ON orthopedic_imaging_links(episode_id);

CREATE TABLE IF NOT EXISTS orthopedic_postop_visits (
  id TEXT PRIMARY KEY,
  episode_id TEXT NOT NULL,
  surgery_id TEXT NOT NULL DEFAULT '',
  visit_date TEXT NOT NULL,
  findings TEXT NOT NULL DEFAULT '',
  complications TEXT NOT NULL DEFAULT '',
  next_step TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_orthopedic_postop_episode ON orthopedic_postop_visits(episode_id);
