-- v3: authored, append-only clinical PDF attachments to the original
-- orthopedic patient episode; PDF bytes are stored in the app's data directory.
CREATE TABLE IF NOT EXISTS physiotherapy_documents (
 id TEXT PRIMARY KEY,
 orthopedic_episode_id TEXT NOT NULL REFERENCES orthopedic_episodes(id),
 client_id TEXT NOT NULL REFERENCES clients(id),
 category TEXT NOT NULL CHECK(category IN ('assessment','progress','discharge','other')),
 display_name TEXT NOT NULL,
 storage_path TEXT NOT NULL UNIQUE CHECK(storage_path LIKE 'rehab-pdf/%'),
 content_sha256 TEXT NOT NULL CHECK(length(content_sha256)=64),
 content_size INTEGER NOT NULL CHECK(content_size BETWEEN 100 AND 20971520),
 source_note TEXT NOT NULL DEFAULT '',
 clinician_id TEXT NOT NULL,
 created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_physio_documents_episode
 ON physiotherapy_documents(orthopedic_episode_id,created_at DESC);
CREATE TRIGGER IF NOT EXISTS physio_document_source_integrity
 BEFORE INSERT ON physiotherapy_documents
 WHEN NOT EXISTS (
  SELECT 1 FROM orthopedic_episodes e WHERE e.id=NEW.orthopedic_episode_id
  AND e.client_id=NEW.client_id AND e.status='active'
 )
 BEGIN SELECT RAISE(ABORT,'Physiotherapy document does not match an active patient episode'); END;
CREATE TRIGGER IF NOT EXISTS physio_document_no_update
 BEFORE UPDATE ON physiotherapy_documents
 BEGIN SELECT RAISE(ABORT,'Clinical document registry is immutable'); END;
CREATE TRIGGER IF NOT EXISTS physio_document_no_delete
 BEFORE DELETE ON physiotherapy_documents
 BEGIN SELECT RAISE(ABORT,'Clinical document history cannot be deleted'); END;
