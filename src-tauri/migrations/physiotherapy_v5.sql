-- v5: anatomical body map observations, independent from diagnostic
-- orthopedic fracture markers and immutable once clinically documented.
CREATE TABLE IF NOT EXISTS physiotherapy_body_markers (
 id TEXT PRIMARY KEY,
 physiotherapy_episode_id TEXT NOT NULL REFERENCES physiotherapy_episodes(id),
 view TEXT NOT NULL CHECK(view IN ('anterior','posterior','lateral')),
 region TEXT NOT NULL CHECK(length(trim(region))>0),
 laterality TEXT NOT NULL CHECK(laterality IN ('left','right','bilateral','midline')),
 classification TEXT NOT NULL CHECK(classification IN ('pain','restriction','weakness','other')),
 x REAL NOT NULL CHECK(x BETWEEN 0 AND 1),
 y REAL NOT NULL CHECK(y BETWEEN 0 AND 1),
 note TEXT NOT NULL DEFAULT '',
 clinician_id TEXT NOT NULL,
 observed_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_physio_body_map_episode
ON physiotherapy_body_markers(physiotherapy_episode_id,observed_at);
CREATE TRIGGER IF NOT EXISTS physio_body_marker_no_update
BEFORE UPDATE ON physiotherapy_body_markers
BEGIN SELECT RAISE(ABORT,'Anatomical findings are append-only'); END;
CREATE TRIGGER IF NOT EXISTS physio_body_marker_no_delete
BEFORE DELETE ON physiotherapy_body_markers
BEGIN SELECT RAISE(ABORT,'Anatomical history cannot be deleted'); END;
CREATE TRIGGER IF NOT EXISTS physio_body_marker_active
BEFORE INSERT ON physiotherapy_body_markers
WHEN NOT EXISTS (
 SELECT 1 FROM physiotherapy_episodes f
 JOIN orthopedic_episodes e ON e.id=f.orthopedic_episode_id
 WHERE f.id=NEW.physiotherapy_episode_id
 AND f.status='active' AND e.status='active'
)
BEGIN SELECT RAISE(ABORT,'Cannot record body marker on inactive case'); END;
