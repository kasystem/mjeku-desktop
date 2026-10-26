-- Dedicated fiscal-only service labels. Never infer the label from clinical notes/title.
CREATE TABLE IF NOT EXISTS sef_mjeku_item_titles (
 visit_item_id TEXT PRIMARY KEY REFERENCES visit_items(id),
 fiscal_title TEXT NOT NULL CHECK(length(trim(fiscal_title)) BETWEEN 3 AND 120),
 created_at TEXT NOT NULL,
 updated_at TEXT NOT NULL
);
CREATE TRIGGER IF NOT EXISTS sef_mjeku_item_title_no_delete
 BEFORE DELETE ON sef_mjeku_item_titles
 BEGIN SELECT RAISE(ABORT,'SEF fiscal service title history cannot be deleted'); END;
CREATE TRIGGER IF NOT EXISTS sef_mjeku_item_title_frozen
 BEFORE UPDATE ON sef_mjeku_item_titles
 WHEN EXISTS(SELECT 1 FROM visit_items WHERE id=OLD.visit_item_id AND fiscalized=1)
 BEGIN SELECT RAISE(ABORT,'SEF fiscal service title is frozen after fiscalization'); END;
